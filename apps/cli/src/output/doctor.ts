function doctorStatusMark(status: string): string {
  switch (status) {
    case "pass":
      return "PASS";
    case "warn":
      return "WARN";
    case "fail":
      return "FAIL";
    default:
      return "SKIP";
  }
}

export function formatSiralosDoctorReport(report: import("@siralos/core").DoctorReport): string {
  const lines: string[] = [
    "Siralos Doctor",
    `Siralos ${report.runtime.version} on Node ${report.runtime.nodeMajor} (${report.runtime.platform})`,
    "",
  ];
  const byArea = new Map<string, import("@siralos/core").DoctorCheckResult[]>();
  for (const check of report.checks) {
    const area = byArea.get(check.area) ?? [];
    area.push(check);
    byArea.set(check.area, area);
  }
  for (const area of report.requestedAreas) {
    const checks = byArea.get(area) ?? [];
    const status = checks.some((check) => check.status === "fail")
      ? "FAIL"
      : checks.some((check) => check.status === "warn")
        ? "WARN"
        : checks.some((check) => check.status === "pass")
          ? "PASS"
          : "SKIP";
    lines.push(`${area.padEnd(15)} ${status}`);
  }
  const warnings = report.checks.filter((check) => check.status === "warn");
  const failures = report.checks.filter((check) => check.status === "fail");
  if (report.counts.total > 0) {
    lines.push(
      "",
      `${report.counts.pass} passed, ${report.counts.warn} warning${report.counts.warn === 1 ? "" : "s"}, ${report.counts.fail} failed, ${report.counts.skip} skipped.`,
    );
  }
  const interesting = [...failures, ...warnings];
  if (interesting.length > 0) {
    lines.push("");
    for (const check of interesting) {
      lines.push(`${check.area}: ${check.summary}`);
      for (const detail of check.details ?? []) {
        lines.push(`- ${detail.label}: ${detail.value}`);
      }
    }
  }
  if (report.snapshot !== null) {
    lines.push(
      "",
      `Capability snapshot: ${report.snapshot.providers.length} provider(s), ${report.snapshot.tools.projectedAvailable} tools available, sandbox ${report.snapshot.sandbox.state}, godot ${report.snapshot.godot.state}.`,
    );
  }
  lines.push("", "Exit codes: 0 = no failures, 1 = one or more failures, 2 = invocation error.");
  return `${lines.join("\n")}\n`;
}

export function formatSafeDoctorReport(report: import("@siralos/core").SafeDoctorReport): string {
  const lines = [
    "Siralos Doctor (safe report)",
    `Siralos ${report.runtime.version} on Node ${report.runtime.nodeMajor} (${report.runtime.platform})`,
    `Schema: ${report.schemaVersion}`,
    `Checks: ${report.counts.pass} passed, ${report.counts.warn} warning${report.counts.warn === 1 ? "" : "s"}, ${report.counts.fail} failed, ${report.counts.skip} skipped.`,
  ];
  for (const check of report.checks) {
    if (check.status === "pass" || check.status === "skip") {
      continue;
    }
    lines.push(`[${doctorStatusMark(check.status)}] ${check.area} ${check.id}: ${check.summary}`);
  }
  for (const category of report.errorCategories) {
    lines.push(`Category: ${category.area} ${category.status} x${category.count}`);
  }
  return `${lines.join("\n")}\n`;
}

export function formatSelfReference(self: import("@siralos/core").SelfReference): string {
  const lines = [
    `${self.name} — installed Siralos runtime`,
    `Version: ${self.runtime.version}`,
    `Node major: ${self.runtime.nodeMajor}`,
    `Platform: ${self.runtime.platform}`,
    `Self-reference revision: ${self.revision}`,
    "",
    "Sections (self.read <section>):",
  ];
  for (const section of self.sections) {
    lines.push(`  ${section.id.padEnd(16)} ${section.title}`);
  }
  lines.push(
    "",
    "This is host-owned documentation of the exact installed build; it is read-only, contains no secrets, and is not model training memory.",
  );
  return `${lines.join("\n")}\n`;
}
