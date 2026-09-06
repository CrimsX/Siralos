//! Estimator-calibration comparison (decision 102, P3 — e1 informational tool).
//!
//! Pure comparison function over recorded requests with captured usage.
//! For each request where `input_tokens` was present, the estimator
//! prediction (via the existing `projection/estimator` path) is compared to
//! the provider-reported real `input_tokens`, grouped per model (token counts
//! are model-specific). Reports bias (mean `predicted/real`), spread,
//! bytes-per-token fit per segment class where separable, and
//! `cached_tokens/input_tokens` ratio where reported.
//!
//! Explicitly labeled **INFORMATIONAL** evidence per decision 94 — no
//! `estimate_tokens` change, no threshold change, no benchmark rerun. The
//! stable-prefix cache rule is measurement-only; this tool never gates.

use std::collections::BTreeMap;

/// One calibration sample: estimator prediction vs provider-reported real usage.
///
/// `predicted_tokens` should be computed via the existing
/// `projection/estimator` path for the same request whose response carried
/// `real_input_tokens`. When `real_input_tokens` is `None` the sample is
/// excluded from calibration (no fabrication — absent stays absent).
#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationSample {
    /// Model identifier for per-model grouping (token counts are model-specific).
    pub model: String,
    /// Estimator prediction for the request's input tokens.
    pub predicted_tokens: usize,
    /// Provider-reported real input tokens where present.
    pub real_input_tokens: Option<u64>,
    /// Total request bytes where separable (for bytes-per-token fit).
    pub request_bytes: Option<usize>,
    /// Segment-class bytes where separable: stable, volatile, contextual.
    pub segment_bytes: Option<SegmentBytes>,
    /// Provider-reported cached tokens where reported.
    pub cached_tokens: Option<u64>,
}

/// Separable segment-class bytes for bytes-per-token fitting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentBytes {
    /// Stable prefix bytes (system instructions, skills, tool definitions).
    pub stable: usize,
    /// Volatile working-set bytes (assembled working set, tool results).
    pub volatile: usize,
}

/// Per-model calibration report (INFORMATIONAL per decision 94).
#[derive(Debug, Clone, PartialEq)]
pub struct PerModelCalibration {
    /// Model identifier.
    pub model: String,
    /// Number of samples with real `input_tokens` present for this model.
    pub sample_count: usize,
    /// Mean `predicted/real` ratio (bias). `None` when no usable samples.
    pub mean_bias: Option<f64>,
    /// Min `predicted/real` ratio.
    pub min_bias: Option<f64>,
    /// Max `predicted/real` ratio.
    pub max_bias: Option<f64>,
    /// Standard deviation of `predicted/real` (population).
    pub std_bias: Option<f64>,
    /// Mean bytes per real token where `request_bytes` separable.
    pub mean_bytes_per_token: Option<f64>,
    /// Per-segment-class bytes-per-token where separable.
    pub segment_fit: Option<PerSegmentFit>,
    /// Mean `cached_tokens/input_tokens` where `cached_tokens` reported.
    pub mean_cached_ratio: Option<f64>,
    /// Number of samples with `cached_tokens` present.
    pub cached_samples: usize,
}

/// Bytes-per-token fit per segment class.
#[derive(Debug, Clone, PartialEq)]
pub struct PerSegmentFit {
    /// Stable bytes per real token.
    pub stable_bpt: f64,
    /// Volatile bytes per real token.
    pub volatile_bpt: f64,
}

/// Full calibration report (INFORMATIONAL).
#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationReport {
    /// Label marking the report as informational evidence per decision 94.
    pub informational: bool,
    /// Total samples passed in (including those without real usage).
    pub total_samples: usize,
    /// Total samples with real `input_tokens` present (used for calibration).
    pub usable_samples: usize,
    /// Total samples without real `input_tokens` (excluded, never fabricated).
    pub excluded_no_usage: usize,
    /// Per-model reports in lexicographic model order.
    pub per_model: Vec<PerModelCalibration>,
    /// Overall mean bias across all usable samples.
    pub overall_mean_bias: Option<f64>,
}

impl CalibrationReport {
    /// Whether the report contains any usable calibration data.
    #[must_use]
    pub fn has_evidence(&self) -> bool {
        self.usable_samples > 0
    }
}

/// Pure comparison function over calibration samples (INFORMATIONAL).
///
/// For each sample with `real_input_tokens == Some`, computes
/// `predicted/real` bias grouped per model, plus bytes-per-token and
/// cached-ratio where separable. Samples with `None` real usage are excluded
/// and counted in `excluded_no_usage` — never fabricated. Deterministic:
/// per-model order is lexicographic, floating-point is stable for identical
/// inputs.
///
/// Returns an informational report that must NOT be used to gate or adjust
/// estimator behavior (decision 94).
#[must_use]
pub fn compare_estimator_calibration(
    samples: &[CalibrationSample],
) -> CalibrationReport {
    let total_samples = samples.len();
    let usable: Vec<&CalibrationSample> =
        samples.iter().filter(|s| s.real_input_tokens.is_some()).collect();
    let usable_samples = usable.len();
    let excluded_no_usage = total_samples - usable_samples;

    // Group usable samples per model (token counts are model-specific).
    let mut by_model: BTreeMap<String, Vec<&CalibrationSample>> =
        BTreeMap::new();
    for sample in &usable {
        by_model.entry(sample.model.clone()).or_default().push(*sample);
    }

    let mut per_model = Vec::new();
    let mut all_biases = Vec::new();

    for (model, group) in by_model {
        let mut biases = Vec::new();
        let mut cached_ratios = Vec::new();
        let mut cached_samples = 0usize;
        let mut bpt_samples: Vec<(usize, u64)> = Vec::new();
        let mut stable_bpt_acc: Vec<(usize, u64)> = Vec::new();
        let mut volatile_bpt_acc: Vec<(usize, u64)> = Vec::new();

        for sample in &group {
            let real = sample.real_input_tokens.expect("usable has real");
            if real == 0 {
                continue;
            }
            let bias = sample.predicted_tokens as f64 / real as f64;
            biases.push(bias);
            all_biases.push(bias);
            if let Some(ct) = sample.cached_tokens {
                cached_samples += 1;
                cached_ratios.push(ct as f64 / real as f64);
            }
            if let Some(bytes) = sample.request_bytes {
                bpt_samples.push((bytes, real));
            }
            if let Some(seg) = &sample.segment_bytes {
                stable_bpt_acc.push((seg.stable, real));
                volatile_bpt_acc.push((seg.volatile, real));
            }
        }

        let sample_count = group.len();
        let (mean_bias, min_bias, max_bias, std_bias) = bias_stats(&biases);
        let mean_bytes_per_token = if bpt_samples.is_empty() {
            None
        } else {
            let total_bytes: usize = bpt_samples.iter().map(|(b, _)| b).sum();
            let total_tokens: u64 = bpt_samples.iter().map(|(_, t)| t).sum();
            if total_tokens == 0 {
                None
            } else {
                Some(total_bytes as f64 / total_tokens as f64)
            }
        };
        let segment_fit = if stable_bpt_acc.is_empty()
            || volatile_bpt_acc.is_empty()
        {
            None
        } else {
            let total_stable: usize =
                stable_bpt_acc.iter().map(|(b, _)| b).sum();
            let total_volatile: usize =
                volatile_bpt_acc.iter().map(|(b, _)| b).sum();
            let total_tokens: u64 =
                stable_bpt_acc.iter().map(|(_, t)| t).sum();
            if total_tokens == 0 {
                None
            } else {
                Some(PerSegmentFit {
                    stable_bpt: total_stable as f64 / total_tokens as f64,
                    volatile_bpt: total_volatile as f64 / total_tokens as f64,
                })
            }
        };
        let mean_cached_ratio = if cached_ratios.is_empty() {
            None
        } else {
            Some(
                cached_ratios.iter().sum::<f64>() / cached_ratios.len() as f64,
            )
        };

        per_model.push(PerModelCalibration {
            model,
            sample_count,
            mean_bias,
            min_bias,
            max_bias,
            std_bias,
            mean_bytes_per_token,
            segment_fit,
            mean_cached_ratio,
            cached_samples,
        });
    }

    let overall_mean_bias = if all_biases.is_empty() {
        None
    } else {
        Some(all_biases.iter().sum::<f64>() / all_biases.len() as f64)
    };

    CalibrationReport {
        informational: true,
        total_samples,
        usable_samples,
        excluded_no_usage,
        per_model,
        overall_mean_bias,
    }
}

fn bias_stats(
    biases: &[f64],
) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    if biases.is_empty() {
        return (None, None, None, None);
    }
    let mean = biases.iter().sum::<f64>() / biases.len() as f64;
    let min = biases.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = biases.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let variance = biases.iter().map(|b| (b - mean).powi(2)).sum::<f64>()
        / biases.len() as f64;
    let std = variance.sqrt();
    (Some(mean), Some(min), Some(max), Some(std))
}

#[cfg(test)]
mod tests {
    use super::{
        CalibrationSample, SegmentBytes, compare_estimator_calibration,
    };

    fn sample(
        model: &str,
        predicted: usize,
        real: Option<u64>,
    ) -> CalibrationSample {
        CalibrationSample {
            model: model.to_owned(),
            predicted_tokens: predicted,
            real_input_tokens: real,
            request_bytes: None,
            segment_bytes: None,
            cached_tokens: None,
        }
    }

    #[test]
    fn bias_mean_predicted_over_real() {
        let samples = vec![
            sample("gpt-4o", 100, Some(100)),
            sample("gpt-4o", 200, Some(100)),
        ];
        let report = compare_estimator_calibration(&samples);
        assert_eq!(report.total_samples, 2);
        assert_eq!(report.usable_samples, 2);
        assert_eq!(report.excluded_no_usage, 0);
        assert!(report.informational);
        let pm = &report.per_model[0];
        // biases: 1.0, 2.0 => mean 1.5
        assert!((pm.mean_bias.unwrap() - 1.5).abs() < 1e-9);
        assert!((pm.min_bias.unwrap() - 1.0).abs() < 1e-9);
        assert!((pm.max_bias.unwrap() - 2.0).abs() < 1e-9);
        // overall mean mirrors single model
        assert!((report.overall_mean_bias.unwrap() - 1.5).abs() < 1e-9);
    }

    #[test]
    fn spread_std_computed() {
        let samples = vec![
            sample("m", 10, Some(10)),
            sample("m", 20, Some(10)),
            sample("m", 30, Some(10)),
        ];
        // biases 1,2,3 => mean 2, variance ((1+0+1)/3)=0.666..., std ~0.816
        let report = compare_estimator_calibration(&samples);
        let pm = &report.per_model[0];
        assert!((pm.mean_bias.unwrap() - 2.0).abs() < 1e-9);
        let std = pm.std_bias.unwrap();
        assert!((std - 0.816_496_580_927_726).abs() < 1e-9);
    }

    #[test]
    fn per_model_grouping() {
        let samples = vec![
            sample("gpt-4o", 100, Some(100)),
            sample("claude-3", 50, Some(100)),
            sample("gpt-4o", 200, Some(100)),
        ];
        let report = compare_estimator_calibration(&samples);
        assert_eq!(report.per_model.len(), 2);
        // Lexicographic order: claude-3, gpt-4o
        assert_eq!(report.per_model[0].model, "claude-3");
        assert_eq!(report.per_model[1].model, "gpt-4o");
        assert_eq!(report.per_model[0].sample_count, 1);
        assert_eq!(report.per_model[1].sample_count, 2);
        assert!((report.per_model[0].mean_bias.unwrap() - 0.5).abs() < 1e-9);
        assert!((report.per_model[1].mean_bias.unwrap() - 1.5).abs() < 1e-9);
    }

    #[test]
    fn bytes_per_token_fit() {
        let samples = vec![
            CalibrationSample {
                model: "m".to_owned(),
                predicted_tokens: 10,
                real_input_tokens: Some(10),
                request_bytes: Some(40),
                segment_bytes: None,
                cached_tokens: None,
            },
            CalibrationSample {
                model: "m".to_owned(),
                predicted_tokens: 20,
                real_input_tokens: Some(10),
                request_bytes: Some(40),
                segment_bytes: None,
                cached_tokens: None,
            },
        ];
        // total bytes 80 / total tokens 20 = 4.0
        let report = compare_estimator_calibration(&samples);
        assert!(
            (report.per_model[0].mean_bytes_per_token.unwrap() - 4.0).abs()
                < 1e-9
        );
    }

    #[test]
    fn bytes_per_token_per_segment_class() {
        let samples = vec![
            CalibrationSample {
                model: "m".to_owned(),
                predicted_tokens: 10,
                real_input_tokens: Some(10),
                request_bytes: Some(100),
                segment_bytes: Some(SegmentBytes { stable: 60, volatile: 40 }),
                cached_tokens: None,
            },
            CalibrationSample {
                model: "m".to_owned(),
                predicted_tokens: 10,
                real_input_tokens: Some(10),
                request_bytes: Some(100),
                segment_bytes: Some(SegmentBytes { stable: 60, volatile: 40 }),
                cached_tokens: None,
            },
        ];
        let report = compare_estimator_calibration(&samples);
        let fit = report.per_model[0].segment_fit.as_ref().unwrap();
        // total stable 120 / total tokens 20 = 6.0, volatile 80/20=4.0
        assert!((fit.stable_bpt - 6.0).abs() < 1e-9);
        assert!((fit.volatile_bpt - 4.0).abs() < 1e-9);
    }

    #[test]
    fn cached_ratio_reported() {
        let samples = vec![
            CalibrationSample {
                model: "m".to_owned(),
                predicted_tokens: 10,
                real_input_tokens: Some(100),
                request_bytes: None,
                segment_bytes: None,
                cached_tokens: Some(50),
            },
            CalibrationSample {
                model: "m".to_owned(),
                predicted_tokens: 10,
                real_input_tokens: Some(100),
                request_bytes: None,
                segment_bytes: None,
                cached_tokens: Some(30),
            },
        ];
        let report = compare_estimator_calibration(&samples);
        // (0.5 + 0.3)/2 = 0.4
        assert!(
            (report.per_model[0].mean_cached_ratio.unwrap() - 0.4).abs()
                < 1e-9
        );
        assert_eq!(report.per_model[0].cached_samples, 2);
    }

    #[test]
    fn no_fabrication_when_usage_absent() {
        let samples = vec![
            sample("m", 100, None),
            sample("m", 200, None),
            sample("m", 300, Some(100)),
        ];
        let report = compare_estimator_calibration(&samples);
        assert_eq!(report.total_samples, 3);
        assert_eq!(report.usable_samples, 1);
        assert_eq!(report.excluded_no_usage, 2);
        assert_eq!(report.per_model.len(), 1);
        assert_eq!(report.per_model[0].sample_count, 1);
        assert!((report.per_model[0].mean_bias.unwrap() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn empty_and_no_usable_reports_none_bias() {
        let report = compare_estimator_calibration(&[]);
        assert_eq!(report.total_samples, 0);
        assert_eq!(report.usable_samples, 0);
        assert!(report.per_model.is_empty());
        assert!(report.overall_mean_bias.is_none());
        assert!(!report.has_evidence());

        let samples = vec![sample("m", 100, None)];
        let report2 = compare_estimator_calibration(&samples);
        assert!(!report2.has_evidence());
        assert!(report2.per_model.is_empty());
        assert!(report2.overall_mean_bias.is_none());
    }

    #[test]
    fn deterministic_order_and_values() {
        let samples = vec![
            sample("b-model", 100, Some(50)),
            sample("a-model", 200, Some(100)),
        ];
        let r1 = compare_estimator_calibration(&samples);
        let r2 = compare_estimator_calibration(&samples);
        assert_eq!(r1, r2);
        // Lexicographic: a-model first
        assert_eq!(r1.per_model[0].model, "a-model");
        assert_eq!(r1.per_model[1].model, "b-model");
    }

    #[test]
    fn fixture_data_calibration() {
        // Simulate fixture: 4 samples, two models, known predictions and reals
        let samples = vec![
            CalibrationSample {
                model: "gpt-4o".to_owned(),
                predicted_tokens: 25,
                real_input_tokens: Some(20),
                request_bytes: Some(80),
                segment_bytes: Some(SegmentBytes { stable: 48, volatile: 32 }),
                cached_tokens: Some(5),
            },
            CalibrationSample {
                model: "gpt-4o".to_owned(),
                predicted_tokens: 50,
                real_input_tokens: Some(40),
                request_bytes: Some(160),
                segment_bytes: Some(SegmentBytes { stable: 96, volatile: 64 }),
                cached_tokens: None,
            },
            CalibrationSample {
                model: "claude-3".to_owned(),
                predicted_tokens: 30,
                real_input_tokens: Some(30),
                request_bytes: Some(120),
                segment_bytes: None,
                cached_tokens: None,
            },
        ];
        let report = compare_estimator_calibration(&samples);
        assert_eq!(report.total_samples, 3);
        assert_eq!(report.usable_samples, 3);
        assert!(report.has_evidence());
        assert!(report.informational);
        // gpt-4o biases: 25/20=1.25, 50/40=1.25 => mean 1.25
        let gpt =
            report.per_model.iter().find(|m| m.model == "gpt-4o").unwrap();
        assert!((gpt.mean_bias.unwrap() - 1.25).abs() < 1e-9);
        // gpt bytes per token: (80+160)/(20+40)=240/60=4.0
        assert!((gpt.mean_bytes_per_token.unwrap() - 4.0).abs() < 1e-9);
        // gpt cached ratio: 5/20=0.25 (only one cached sample)
        assert!((gpt.mean_cached_ratio.unwrap() - 0.25).abs() < 1e-9);
        assert_eq!(gpt.cached_samples, 1);
    }
}
