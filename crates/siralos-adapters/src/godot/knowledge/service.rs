//! Version-matched Godot API knowledge service.
//!
//! `refresh` regenerates the exact-engine API documentation profile in a
//! Siralos-private probe directory and replaces the loaded knowledge base
//! only after a successful complete generation. On this stage generation
//! fails closed (the runner never spawns the executable), so production
//! always reports `unavailable` and no probe directory is ever created.
//! `search` and `lookup` serve bounded structured results from a loaded
//! base and never expose the raw dump.

use siralos_godot::godot::{
    GodotApiSearchQuery, GodotKnowledgeBase, GodotKnowledgeLookupOutcome,
    GodotKnowledgeQueryResult, GodotKnowledgeRefreshResult,
    GodotKnowledgeStatus, GodotKnowledgeSupport, KNOWLEDGE_SCHEMA_VERSION,
    KnowledgeLookupStatus, KnowledgeQueryStatus, KnowledgeRefreshStatus,
    KnowledgeState, KnowledgeSupportState, classify_godot_manual_channel,
};

use super::api_index::{lookup_godot_api_symbol, search_godot_api_index};
use crate::godot::process::godot_knowledge_runner::GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE;
use crate::godot::process::version_parser::parse_godot_version_text;

const MAX_QUERY_LENGTH: usize = 4096;
const MAX_SYMBOL_LENGTH: usize = 1024;

const NO_BASE_LOADED_MESSAGE: &str = "No Godot API knowledge is loaded: exact-engine API generation is unavailable on this platform.";

/// Fail-closed knowledge service over an optionally loaded base.
#[derive(Debug, Clone)]
pub struct GodotKnowledgeService {
    platform: String,
    loaded_base: Option<GodotKnowledgeBase>,
}

impl GodotKnowledgeService {
    /// Create the production service: nothing loaded, generation closed.
    pub fn new(platform: impl Into<String>) -> Self {
        Self { platform: platform.into(), loaded_base: None }
    }

    /// Test seam: start with a fully loaded knowledge base; production
    /// never supplies one.
    pub fn with_loaded_base(
        platform: impl Into<String>,
        loaded_base: GodotKnowledgeBase,
    ) -> Self {
        Self { platform: platform.into(), loaded_base: Some(loaded_base) }
    }

    /// Truthful platform-level support state.
    pub fn support(&self) -> GodotKnowledgeSupport {
        GodotKnowledgeSupport {
            state: KnowledgeSupportState::Unavailable,
            reason: Some(
                GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE.to_owned(),
            ),
            platform: self.platform.clone(),
        }
    }

    /// Regenerate the knowledge profile; refuses before any effect.
    pub fn refresh(&mut self, cancelled: bool) -> GodotKnowledgeRefreshResult {
        if cancelled {
            return GodotKnowledgeRefreshResult::NotReady {
                status: KnowledgeRefreshStatus::Cancelled,
                message: "API knowledge generation was cancelled.".to_owned(),
            };
        }
        GodotKnowledgeRefreshResult::NotReady {
            status: KnowledgeRefreshStatus::Unavailable,
            message: GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE.to_owned(),
        }
    }

    /// Bounded literal/token API search over the loaded base.
    pub fn search(
        &self,
        query: &GodotApiSearchQuery,
        cancelled: bool,
    ) -> GodotKnowledgeQueryResult {
        if cancelled {
            return GodotKnowledgeQueryResult::NotReady {
                status: KnowledgeQueryStatus::Cancelled,
                message: "API search was cancelled.".to_owned(),
            };
        }
        if query.query.trim().is_empty() {
            return GodotKnowledgeQueryResult::NotReady {
                status: KnowledgeQueryStatus::InvalidInput,
                message: "A non-empty query is required.".to_owned(),
            };
        }
        if utf16_len(&query.query) > MAX_QUERY_LENGTH as u64 {
            return GodotKnowledgeQueryResult::NotReady {
                status: KnowledgeQueryStatus::InvalidInput,
                message: format!(
                    "The query exceeds the {MAX_QUERY_LENGTH}-character bound."
                ),
            };
        }
        let Some(loaded_base) = &self.loaded_base else {
            return GodotKnowledgeQueryResult::NotReady {
                status: KnowledgeQueryStatus::Unavailable,
                message: NO_BASE_LOADED_MESSAGE.to_owned(),
            };
        };
        let outcome = search_godot_api_index(
            &loaded_base.index,
            &query.query,
            query.kinds.as_deref(),
            query.limit,
        );
        GodotKnowledgeQueryResult::Ready {
            engine_version: loaded_base.profile.engine.godot_version.clone(),
            results: outcome.results,
            truncated: outcome.truncated,
        }
    }

    /// Exact-symbol API lookup over the loaded base.
    pub fn lookup(
        &self,
        symbol: &str,
        cancelled: bool,
    ) -> GodotKnowledgeLookupOutcome {
        if cancelled {
            return GodotKnowledgeLookupOutcome::NotReady {
                status: KnowledgeLookupStatus::Cancelled,
                message: "API lookup was cancelled.".to_owned(),
            };
        }
        if symbol.trim().is_empty() {
            return GodotKnowledgeLookupOutcome::NotReady {
                status: KnowledgeLookupStatus::InvalidInput,
                message: "A non-empty symbol identity is required.".to_owned(),
            };
        }
        if utf16_len(symbol) > MAX_SYMBOL_LENGTH as u64 {
            return GodotKnowledgeLookupOutcome::NotReady {
                status: KnowledgeLookupStatus::InvalidInput,
                message: format!(
                    "The symbol identity exceeds the {MAX_SYMBOL_LENGTH}-character bound."
                ),
            };
        }
        let Some(loaded_base) = &self.loaded_base else {
            return GodotKnowledgeLookupOutcome::NotReady {
                status: KnowledgeLookupStatus::Unavailable,
                message: NO_BASE_LOADED_MESSAGE.to_owned(),
            };
        };
        let result = lookup_godot_api_symbol(&loaded_base.index, symbol);
        match result {
            None => GodotKnowledgeLookupOutcome::NotReady {
                status: KnowledgeLookupStatus::NotFound,
                message: format!("Unknown API symbol {symbol}."),
            },
            Some(result) => GodotKnowledgeLookupOutcome::Ready {
                engine_version: loaded_base
                    .profile
                    .engine
                    .godot_version
                    .clone(),
                result: Box::new(result),
            },
        }
    }

    /// Bounded in-memory knowledge state for CLI diagnostics.
    pub fn status(&self) -> GodotKnowledgeStatus {
        if let Some(loaded_base) = &self.loaded_base {
            let version = parse_godot_version_text(
                &loaded_base.profile.engine.godot_version,
            )
            .ok();
            let manual_channel =
                version.as_ref().map(classify_godot_manual_channel);
            return GodotKnowledgeStatus {
                state: KnowledgeState::Ready,
                reason: None,
                platform: self.platform.clone(),
                profile: Some(loaded_base.profile.clone()),
                cache_enabled: false,
                schema_version: KNOWLEDGE_SCHEMA_VERSION,
                manual_channel,
            };
        }
        GodotKnowledgeStatus {
            state: KnowledgeState::Unavailable,
            reason: Some(
                GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE.to_owned(),
            ),
            platform: self.platform.clone(),
            profile: None,
            cache_enabled: false,
            schema_version: KNOWLEDGE_SCHEMA_VERSION,
            manual_channel: None,
        }
    }
}

fn utf16_len(text: &str) -> u64 {
    text.chars().map(char::len_utf16).sum::<usize>() as u64
}

#[cfg(test)]
mod tests {
    use super::GodotKnowledgeService;
    use crate::godot::knowledge::{
        build_godot_api_index, parse_godot_api_dump_with_docs,
    };
    use crate::godot::process::godot_knowledge_runner::GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE;
    use siralos_godot::godot::{
        GodotApiSearchQuery, GodotKnowledgeLookupOutcome,
        GodotKnowledgeQueryResult, GodotKnowledgeRefreshResult, KnowledgeApi,
        KnowledgeEngine, KnowledgeIndex, KnowledgeLookupStatus,
        KnowledgeQueryStatus, KnowledgeRefreshStatus, KnowledgeState,
        KnowledgeSupportState,
    };

    const DUMP_JSON: &str = r#"{
  "header": {"version_full_name": "4.7.1-stable", "hash": "abc123"},
  "classes": [
    {
      "name": "Node",
      "base_class": "Object",
      "brief_description": "Base class for the scene tree.",
      "methods": [
        {"name": "add_child", "return_type": "void",
         "arguments": [{"name": "node", "type": "Node", "default_value": null}],
         "is_vararg": true, "hash": 12345678,
         "description": "Adds a child node."}
      ],
      "properties": [
        {"name": "name", "type": "StringName", "setter": "set_name",
         "getter": "get_name", "description": "The name."}
      ],
      "signals": [{"name": "ready", "arguments": []}],
      "constants": [
        {"name": "NOTIFICATION_READY", "value": 13, "description": null}
      ]
    }
  ],
  "builtin_classes": [],
  "global_constants": [{"name": "PI", "value": 3.14}],
  "global_enums": [],
  "utility_functions": []
}"#;

    fn loaded_service() -> GodotKnowledgeService {
        let document = parse_godot_api_dump_with_docs(DUMP_JSON.as_bytes())
            .expect("fixture parses");
        let index = build_godot_api_index(&document).expect("index builds");
        let profile = siralos_godot::godot::GodotKnowledgeProfileV1 {
            version: 1,
            engine: KnowledgeEngine {
                installation_id: "path-1".to_owned(),
                executable_sha256: "a".repeat(64),
                godot_version: "4.7.1.stable".to_owned(),
                edition: "standard".to_owned(),
            },
            api: KnowledgeApi {
                dump_sha256: index.dump_sha256.clone(),
                generated_at: "2026-01-01T00:00:00Z".to_owned(),
                class_count: 1,
                builtin_class_count: 0,
                utility_function_count: 0,
                global_enum_count: 0,
                global_constant_count: 1,
            },
            index: KnowledgeIndex {
                schema_version: 1,
                symbol_count: index.symbols.len(),
            },
        };
        GodotKnowledgeService::with_loaded_base(
            "win32",
            siralos_godot::godot::GodotKnowledgeBase { profile, index },
        )
    }

    #[test]
    fn support_and_status_report_unavailable_when_closed() {
        let service = GodotKnowledgeService::new("win32");
        let support = service.support();
        assert_eq!(support.state, KnowledgeSupportState::Unavailable);
        assert_eq!(
            support.reason.as_deref(),
            Some(GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE)
        );
        let status = service.status();
        assert_eq!(status.state, KnowledgeState::Unavailable);
        assert!(!status.cache_enabled);
        assert_eq!(status.schema_version, 1);
        assert_eq!(status.manual_channel, None);
    }

    #[test]
    fn refresh_reports_unavailable_without_effects() {
        let mut service = GodotKnowledgeService::new("win32");
        let result = service.refresh(false);
        assert_eq!(
            result,
            GodotKnowledgeRefreshResult::NotReady {
                status: KnowledgeRefreshStatus::Unavailable,
                message: GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE
                    .to_owned()
            }
        );
        let cancelled = service.refresh(true);
        assert_eq!(
            cancelled,
            GodotKnowledgeRefreshResult::NotReady {
                status: KnowledgeRefreshStatus::Cancelled,
                message: "API knowledge generation was cancelled.".to_owned()
            }
        );
    }

    #[test]
    fn search_validates_input_before_availability() {
        let service = GodotKnowledgeService::new("win32");
        let empty = service.search(
            &GodotApiSearchQuery {
                query: "   ".to_owned(),
                kinds: None,
                limit: None,
            },
            false,
        );
        assert_eq!(
            empty,
            GodotKnowledgeQueryResult::NotReady {
                status: KnowledgeQueryStatus::InvalidInput,
                message: "A non-empty query is required.".to_owned()
            }
        );
        let oversize = service.search(
            &GodotApiSearchQuery {
                query: "a".repeat(4097),
                kinds: None,
                limit: None,
            },
            false,
        );
        assert_eq!(
            oversize,
            GodotKnowledgeQueryResult::NotReady {
                status: KnowledgeQueryStatus::InvalidInput,
                message: "The query exceeds the 4096-character bound."
                    .to_owned()
            }
        );
        let unavailable = service.search(
            &GodotApiSearchQuery {
                query: "node".to_owned(),
                kinds: None,
                limit: None,
            },
            false,
        );
        assert_eq!(
            unavailable,
            GodotKnowledgeQueryResult::NotReady {
                status: KnowledgeQueryStatus::Unavailable,
                message: super::NO_BASE_LOADED_MESSAGE.to_owned()
            }
        );
    }

    #[test]
    fn search_serves_deterministic_results_from_the_loaded_base() {
        let service = loaded_service();
        let exact = service.search(
            &GodotApiSearchQuery {
                query: "add_child".to_owned(),
                kinds: None,
                limit: None,
            },
            false,
        );
        let GodotKnowledgeQueryResult::Ready {
            engine_version,
            results,
            truncated,
        } = exact
        else {
            panic!("expected ready");
        };
        assert_eq!(engine_version, "4.7.1.stable");
        assert!(!truncated);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol, "class:Node/method:add_child");
        assert_eq!(results[0].summary, "Adds a child node.");
    }

    #[test]
    fn lookup_serves_details_and_structured_not_found() {
        let service = loaded_service();
        let found = service.lookup("class:Node/method:add_child", false);
        let GodotKnowledgeLookupOutcome::Ready { result, .. } = found else {
            panic!("expected ready");
        };
        assert_eq!(
            result.signature.as_deref(),
            Some("vararg add_child(node: Node) -> void")
        );
        assert_eq!(result.details.hash.as_deref(), Some("12345678"));
        let unknown = service.lookup("class:Ghost", false);
        assert_eq!(
            unknown,
            GodotKnowledgeLookupOutcome::NotReady {
                status: KnowledgeLookupStatus::NotFound,
                message: "Unknown API symbol class:Ghost.".to_owned()
            }
        );
    }

    #[test]
    fn status_is_ready_with_manual_channel_when_loaded() {
        let service = loaded_service();
        let status = service.status();
        assert_eq!(status.state, KnowledgeState::Ready);
        assert!(status.profile.is_some());
        assert_eq!(status.manual_channel.as_deref(), Some("4.7"));
    }
}
