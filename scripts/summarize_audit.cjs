// Render cargo-audit's complete report into the Actions job summary. This is
// reporting only while existing vulnerabilities await dependency migrations.
const fs = require('node:fs');

const report = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const findings = report.vulnerabilities?.list;
if (!Array.isArray(findings) || !report.database || !report.lockfile) {
  throw new Error('cargo-audit did not produce a complete vulnerability report');
}

const cell = (value) => String(value).replace(/[\r\n]+/g, ' ').replace(/\|/g, '\\|');
const lines = [
  '## Dependency security audit',
  '',
  `**${findings.length} known vulnerability findings.** This job reports findings; it does not enforce a clean audit yet.`,
  'The full report, including informational warnings, is attached as the dependency-audit artifact.',
  '',
];

if (findings.length) {
  lines.push('| Advisory | Locked dependency | Patched versions |', '| --- | --- | --- |');
  for (const { advisory, package: pkg, versions } of findings) {
    lines.push(`| ${cell(advisory.id)} | ${cell(pkg.name)} ${cell(pkg.version)} | ${cell(versions.patched.join(', ') || 'No patch available')} |`);
  }
}

const summary = `${lines.join('\n')}\n`;
process.stdout.write(summary);
if (process.env.GITHUB_STEP_SUMMARY) {
  fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
}
