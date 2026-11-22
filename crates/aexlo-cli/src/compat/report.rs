//! `aexlo compat`'s reports: text, with colors, symbols and bars on a
//! terminal, or JSON.

use std::collections::BTreeMap;

use anstyle::{AnsiColor, Style};

use super::{Change, PluginReport, Scenario, Status};

/// What changed since the baseline, when one was given.
pub struct Changes {
	pub worse: Vec<Change>,
	pub better: Vec<Change>,
	/// Coverage before and now, over the scenarios both runs have; `None`
	/// when they share none that apply.
	pub coverage: Option<(f64, f64)>,
}

/// The share of the scenarios that apply (all but n/a and invalid) that
/// were ok. `None` when none apply.
pub fn coverage<'a>(statuses: impl IntoIterator<Item = &'a Status>) -> Option<f64> {
	let (mut ok, mut apply) = (0usize, 0usize);
	for status in statuses {
		match status {
			Status::NotApplicable | Status::Invalid => {}
			Status::Ok => {
				ok += 1;
				apply += 1;
			}
			_ => apply += 1,
		}
	}
	(apply > 0).then(|| ok as f64 / apply as f64)
}

/// `94.2%`, rounded down so that only every scenario ok reads `100.0%`.
fn percent(ratio: f64) -> String {
	format!("{:.1}%", (ratio * 1000.0).floor() / 10.0)
}

/// The coverage in green from 95%, yellow from 50%, red below.
fn coverage_style(ratio: f64) -> Style {
	let color = |c: AnsiColor| Style::new().fg_color(Some(c.into())).bold();
	match ratio {
		r if r >= 0.95 => color(AnsiColor::Green),
		r if r >= 0.5 => color(AnsiColor::Yellow),
		_ => color(AnsiColor::Red),
	}
}

/// A coverage figure, right-aligned to `width`, colored when rich.
fn coverage_text(look: Look, ratio: Option<f64>, width: usize) -> String {
	match ratio {
		Some(r) => look.paint(coverage_style(r), &format!("{:>width$}", percent(r))),
		None => format!("{:>width$}", "-"),
	}
}

/// The coverage of a plugin's scenarios.
fn plugin_coverage(report: &PluginReport) -> Option<f64> {
	coverage(report.all().map(|s| &s.status))
}

/// How the text report looks: colors, symbols and bars, or plain text for
/// pipes, files and logs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Look {
	pub rich: bool,
}

impl Look {
	pub const PLAIN: Self = Self { rich: false };
	pub const RICH: Self = Self { rich: true };

	/// `text` in `style`, when rich.
	fn paint(self, style: Style, text: &str) -> String {
		if self.rich && !text.is_empty() {
			format!("{style}{text}{style:#}")
		} else {
			text.to_string()
		}
	}

	fn status(self, status: Status, text: &str) -> String {
		self.paint(style(status), text)
	}

	fn dim(self, text: &str) -> String {
		self.paint(DIM, text)
	}

	fn bold(self, text: &str) -> String {
		self.paint(BOLD, text)
	}

	/// The symbol before a status, with its space; nothing when plain.
	fn symbol(self, status: Status) -> String {
		match self.rich {
			true => format!("{} ", self.status(status, symbol(status))),
			false => String::new(),
		}
	}
}

const DIM: Style = Style::new().dimmed();
const BOLD: Style = Style::new().bold();

fn style(status: Status) -> Style {
	let color = |c: AnsiColor| Style::new().fg_color(Some(c.into()));
	match status {
		Status::Ok => color(AnsiColor::Green),
		Status::Partial => color(AnsiColor::Yellow),
		Status::NotApplicable => DIM,
		Status::Invalid => color(AnsiColor::Magenta),
		Status::Error => color(AnsiColor::Red),
		Status::Timeout => color(AnsiColor::BrightRed),
		Status::Crash => color(AnsiColor::Red).bold(),
	}
}

fn symbol(status: Status) -> &'static str {
	match status {
		Status::Ok => "✓",
		Status::Partial => "◐",
		Status::NotApplicable => "○",
		Status::Invalid => "?",
		Status::Error => "✗",
		Status::Timeout => "◷",
		Status::Crash => "✗",
	}
}

fn counts<'a>(scenarios: impl Iterator<Item = &'a Scenario>) -> BTreeMap<Status, usize> {
	let mut counts = BTreeMap::new();
	for scenario in scenarios {
		*counts.entry(scenario.status).or_insert(0) += 1;
	}
	counts
}

/// `4 ok · 145 crash`, each in its status's color when rich.
fn counts_text(look: Look, counts: &BTreeMap<Status, usize>) -> String {
	Status::ALL
		.iter()
		.filter_map(|s| counts.get(s).map(|n| look.status(*s, &format!("{n} {}", s.name()))))
		.collect::<Vec<_>>()
		.join(&look.dim(" · "))
}

/// A bar `width` cells long, split between the statuses in proportion to
/// `counts`. Every status present gets at least one cell.
fn bar(look: Look, counts: &BTreeMap<Status, usize>, width: usize) -> String {
	let total: usize = counts.values().sum();
	if total == 0 {
		return " ".repeat(width);
	}
	let mut cells: Vec<(Status, usize)> = Status::ALL
		.iter()
		.filter_map(|s| counts.get(s).map(|n| (*s, (n * width).div_ceil(total).max(1))))
		.collect();
	// Rounding up overshoots: take the excess from the widest segments.
	while cells.iter().map(|c| c.1).sum::<usize>() > width {
		let widest = cells.iter_mut().max_by_key(|c| c.1).expect("non-empty");
		if widest.1 <= 1 {
			break;
		}
		widest.1 -= 1;
	}
	cells
		.iter()
		.map(|(status, n)| {
			let glyph = if *status == Status::NotApplicable { "░" } else { "█" };
			look.status(*status, &glyph.repeat(*n))
		})
		.collect()
}

/// Every gap, with the plugins that hit it, most widespread first.
pub fn gaps(reports: &[PluginReport]) -> Vec<(String, Vec<String>)> {
	let mut by_label: BTreeMap<String, Vec<String>> = BTreeMap::new();
	for report in reports {
		for scenario in report.all() {
			for gap in &scenario.unsupported {
				// Where it was asked is detail; what is missing is the gap.
				let key = match gap.detail.is_empty() {
					true => format!("{} {}", gap.kind, gap.name),
					false => format!("{} {} {}", gap.kind, gap.name, gap.detail),
				};
				let plugins = by_label.entry(key).or_default();
				if !plugins.contains(&report.plugin) {
					plugins.push(report.plugin.clone());
				}
			}
		}
	}
	let mut gaps: Vec<_> = by_label.into_iter().collect();
	gaps.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
	gaps
}

/// What makes two scenarios fail the same way: their status, the first line
/// of their message, and what they asked for.
fn signature(scenario: &Scenario) -> (Status, String, Vec<String>) {
	let message = match scenario.status {
		Status::Ok => String::new(),
		_ => scenario
			.message
			.as_deref()
			.and_then(|m| m.lines().next())
			.unwrap_or_default()
			.to_string(),
	};
	let gaps = scenario
		.unsupported
		.iter()
		.map(|g| match g.count {
			0 | 1 => format!("{} (in {})", g.label(), g.command),
			n => format!("{} (in {} ×{n})", g.label(), g.command),
		})
		.collect();
	(scenario.status, message, gaps)
}

/// How much a status needs looking at, for ordering the report: ok and n/a
/// not at all.
fn severity(status: Status) -> u8 {
	match status {
		Status::Ok | Status::NotApplicable => 0,
		Status::Invalid => 1,
		Status::Partial => 2,
		Status::Error => 3,
		Status::Timeout => 4,
		Status::Crash => 5,
	}
}

/// A plugin's worst status, `ok` when nothing needs looking at.
fn worst(report: &PluginReport) -> Status {
	report
		.all()
		.map(|s| s.status)
		.max_by_key(|s| severity(*s))
		.filter(|s| severity(*s) > 0)
		.unwrap_or(Status::Ok)
}

/// A plugin's scenarios grouped by [`signature`], worst first, larger groups
/// before smaller. Without `all`, ok and n/a scenarios are left out.
fn groups(report: &PluginReport, all: bool) -> Vec<Vec<&Scenario>> {
	let mut groups: Vec<Vec<&Scenario>> = Vec::new();
	for scenario in report.all() {
		if !all && severity(scenario.status) == 0 {
			continue;
		}
		let key = signature(scenario);
		match groups.iter_mut().find(|g| signature(g[0]) == key) {
			Some(group) => group.push(scenario),
			None => groups.push(vec![scenario]),
		}
	}
	// Stable, so equal groups keep the order the scenarios ran in.
	groups.sort_by_key(|g| (std::cmp::Reverse(severity(g[0].status)), std::cmp::Reverse(g.len())));
	groups
}

/// At most this many ids are named for a group of scenarios, without `--all`.
const NAMED: usize = 4;

/// Lines of the text report are wrapped at this width.
pub const WIDTH: usize = 100;

/// The width of the bar under the totals, and of each plugin's.
const TOTAL_BAR: usize = 60;
const PLUGIN_BAR: usize = 24;

/// `items` joined with `, `, wrapped at [`WIDTH`], each line after `indent`.
fn wrap(items: &[String], indent: &str) -> Vec<String> {
	let width = |text: &str| text.chars().count();
	let mut lines = Vec::new();
	let mut line = String::new();
	for (i, item) in items.iter().enumerate() {
		let piece = if i + 1 < items.len() {
			format!("{item},")
		} else {
			item.clone()
		};
		if !line.is_empty() && width(indent) + width(&line) + 1 + width(&piece) > WIDTH {
			lines.push(format!("{indent}{line}"));
			line.clear();
		}
		if !line.is_empty() {
			line.push(' ');
		}
		line.push_str(&piece);
	}
	if !line.is_empty() {
		lines.push(format!("{indent}{line}"));
	}
	lines
}

/// A group: its status, how many scenarios, why (the message, then what it
/// asked for), and which scenarios.
///
/// ```text
///     crash      ×35  worker killed by signal 6 (SIGABRT) during PF_Cmd_SMART_RENDER
///                     render:auto@8, render:smart@8, render:smart@32, input:solid, … (+31)
/// ```
///
/// Rich, the status has its symbol and color, and the ids are dimmed.
fn write_group(out: &mut Vec<String>, look: Look, group: &[&Scenario], all: bool) {
	let (status, message, gaps) = signature(group[0]);
	let mut reasons: Vec<String> = Vec::new();
	if !message.is_empty() {
		reasons.push(message);
	}
	reasons.extend(gaps);
	let head = format!(
		"    {}{} {:>5}  ",
		look.symbol(status),
		look.status(status, &format!("{:<8}", status.name())),
		format!("×{}", group.len())
	);
	let body = " ".repeat(if look.rich { 22 } else { 20 });
	let mut ids: Vec<String> = group.iter().map(|s| s.id.clone()).collect();
	if !all && ids.len() > NAMED {
		let more = ids.len() - NAMED;
		ids.truncate(NAMED);
		ids.push(format!("… (+{more})"));
	}
	let mut ids: Vec<String> = wrap(&ids, "")
		.into_iter()
		.map(|line| format!("{body}{}", look.dim(&line)))
		.collect();
	match reasons.split_first() {
		Some((first, rest)) => {
			out.push(format!("{head}{first}"));
			out.extend(rest.iter().map(|r| format!("{body}{r}")));
		}
		// Nothing to say why (an ok group with --all): the ids go first.
		None => {
			let first = ids.remove(0);
			out.push(format!("{head}{}", first.trim_start()));
		}
	}
	out.extend(ids);
}

/// The text report.
///
/// A summary, then each plugin that had problems, worst first, with its
/// scenarios grouped by how they went; then the plugins where everything
/// went well, on one wrapped list; then every gap, most widespread first;
/// then what changed since the baseline. `all` lists every plugin and every
/// scenario. Rich, the totals and each plugin get a bar of their statuses,
/// and statuses their colors and symbols.
pub fn human(reports: &[PluginReport], all: bool, changes: Option<&Changes>, look: Look) -> String {
	let mut out: Vec<String> = Vec::new();
	let total = counts(reports.iter().flat_map(PluginReport::all));
	let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
	let plugins = plural(reports.len(), "plugin", "plugins");
	let scenarios = plural(total.values().sum::<usize>(), "scenario", "scenarios");
	let covered = coverage(reports.iter().flat_map(PluginReport::all).map(|s| &s.status));
	let apply: usize = total
		.iter()
		.filter(|(s, _)| !matches!(s, Status::NotApplicable | Status::Invalid))
		.map(|(_, n)| n)
		.sum();
	let ok = total.get(&Status::Ok).copied().unwrap_or(0);
	if look.rich {
		out.push(format!("{}  {plugins} · {scenarios}", look.bold("aexlo compat")));
		out.push(format!(
			"{}  {} {}",
			bar(look, &total, TOTAL_BAR),
			coverage_text(look, covered, 0),
			look.dim(&format!("coverage ({ok} of {apply} ok)"))
		));
		out.push(counts_text(look, &total));
	} else {
		out.push(format!(
			"aexlo compat: {plugins}, {scenarios}: {}",
			counts_text(look, &total)
		));
		if let Some(covered) = covered {
			out.push(format!(
				"Coverage: {} ({ok} of the {apply} scenarios that apply ran ok)",
				percent(covered)
			));
		}
	}

	let mut order: Vec<&PluginReport> = reports.iter().collect();
	order.sort_by_key(|r| std::cmp::Reverse(severity(worst(r))));
	let (listed, clean): (Vec<&PluginReport>, Vec<&PluginReport>) =
		order.into_iter().partition(|r| all || severity(worst(r)) > 0);
	let name_width = listed.iter().map(|r| r.plugin.chars().count()).max().unwrap_or(0);
	for report in listed {
		out.push(String::new());
		let loaded = matches!(report.load.status, Status::Ok | Status::Partial);
		let worst = worst(report);
		out.push(match (look.rich, loaded) {
			(false, true) => format!(
				"{}  {}  {}",
				report.plugin,
				coverage_text(look, plugin_coverage(report), 0),
				counts_text(look, &counts(report.all()))
			),
			(false, false) => format!("{}  did not load", report.plugin),
			(true, true) => format!(
				"{}{}  {}  {}  {}",
				look.symbol(worst),
				look.bold(&format!("{:<name_width$}", report.plugin)),
				bar(look, &counts(report.all()), PLUGIN_BAR),
				coverage_text(look, plugin_coverage(report), 6),
				counts_text(look, &counts(report.all()))
			),
			(true, false) => format!(
				"{}{}  {}",
				look.symbol(worst),
				look.bold(&format!("{:<name_width$}", report.plugin)),
				look.status(worst, "did not load")
			),
		});
		for group in groups(report, all) {
			write_group(&mut out, look, &group, all);
		}
	}
	if !clean.is_empty() {
		out.push(String::new());
		let title = format!("Everything ok or n/a ({}):", clean.len());
		out.push(match look.rich {
			true => format!("{}{}", look.symbol(Status::Ok), look.status(Status::Ok, &title)),
			false => title,
		});
		let names: Vec<String> = clean.iter().map(|r| r.plugin.clone()).collect();
		out.extend(wrap(&names, "    "));
	}

	let gaps = gaps(reports);
	if !gaps.is_empty() {
		out.push(String::new());
		out.push(look.bold("Asked for, but not done by aexlo (plugins that asked):"));
		for (label, plugins) in gaps {
			let count = look.status(Status::Partial, &format!("{:>3}", plugins.len()));
			let list = format!("[{}]", plugins.join(", "));
			let plain = format!("  {:>3}  {label}  {list}", plugins.len());
			if plain.chars().count() <= WIDTH {
				out.push(format!("  {count}  {label}  {}", look.dim(&list)));
			} else {
				out.push(format!("  {count}  {label}"));
				out.extend(
					wrap(&plugins, "")
						.into_iter()
						.map(|line| format!("         {}", look.dim(&line))),
				);
			}
		}
	}

	if let Some(Changes {
		worse,
		better,
		coverage,
	}) = changes
	{
		out.push(String::new());
		if let Some((was, now)) = coverage {
			let delta = (now - was) * 100.0;
			out.push(format!(
				"Coverage since the baseline: {} -> {} ({}{:.1} points, on the scenarios both runs have)",
				percent(*was),
				coverage_text(look, Some(*now), 0),
				if delta >= 0.0 { "+" } else { "" },
				delta
			));
		}
		if worse.is_empty() && better.is_empty() {
			out.push("Same as the baseline.".into());
		}
		for (title, list, sign) in [
			("Worse than the baseline:", worse, Status::Crash),
			("Better than the baseline:", better, Status::Ok),
		] {
			if list.is_empty() {
				continue;
			}
			out.push(match look.rich {
				true => format!("{}{}", look.symbol(sign), look.status(sign, title)),
				false => title.into(),
			});
			for c in list {
				out.push(format!(
					"  {}  {}: {} -> {}",
					c.plugin,
					c.id,
					look.status(c.was, c.was.name()),
					look.status(c.now, c.now.name())
				));
			}
		}
	}
	let mut text = out.join("\n");
	text.push('\n');
	text
}

/// The JSON report.
pub fn json(reports: &[PluginReport], changes: Option<&Changes>) -> String {
	let total = counts(reports.iter().flat_map(PluginReport::all));
	let summary: BTreeMap<&str, usize> = total.iter().map(|(s, n)| (s.name(), *n)).collect();
	let gaps: Vec<serde_json::Value> = gaps(reports)
		.into_iter()
		.map(|(gap, plugins)| serde_json::json!({ "gap": gap, "plugins": plugins }))
		.collect();
	let mut plugins = serde_json::json!(reports);
	if let Some(list) = plugins.as_array_mut() {
		for (value, report) in list.iter_mut().zip(reports) {
			value["coverage"] = serde_json::json!(plugin_coverage(report));
		}
	}
	let mut value = serde_json::json!({
		"summary": summary,
		"coverage": coverage(reports.iter().flat_map(PluginReport::all).map(|s| &s.status)),
		"plugins": plugins,
		"gaps": gaps,
	});
	if let Some(Changes {
		worse,
		better,
		coverage,
	}) = changes
	{
		value["worse"] = serde_json::json!(worse);
		value["better"] = serde_json::json!(better);
		value["coverage_change"] = match coverage {
			Some((was, now)) => serde_json::json!({ "was": was, "now": now }),
			None => serde_json::Value::Null,
		};
	}
	serde_json::to_string_pretty(&value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
	use super::super::tests::{report, scenario};
	use super::*;

	fn reports() -> Vec<PluginReport> {
		let crash = |id: &str| Scenario {
			message: Some("worker killed by signal 11 (SIGSEGV) during PF_Cmd_RENDER".into()),
			..scenario(id, Status::Crash)
		};
		let mut erring = report(&[("render:auto@8", Status::Error)]);
		erring.plugin = "Erring".into();
		erring.scenarios[0].message = Some("Plugin rejected Render with error code: 516".into());
		let mut crashing = report(&[]);
		crashing.plugin = "Crashing".into();
		crashing.scenarios = (0..6).map(|i| crash(&format!("size:{i}"))).collect();
		let mut clean = report(&[("render:auto@8", Status::Ok), ("render:gpu@8", Status::NotApplicable)]);
		clean.plugin = "Clean".into();
		let mut unloaded = report(&[]);
		unloaded.plugin = "Unloaded".into();
		unloaded.load = Scenario {
			message: Some("Plugin not found: Unloaded.plugin".into()),
			..scenario("load", Status::Error)
		};
		vec![clean, erring, crashing, unloaded]
	}

	#[test]
	fn the_text_report_lists_problems_worst_first_and_folds_the_rest() {
		let text = human(&reports(), false, None, Look::PLAIN);
		let expected = "\
aexlo compat: 4 plugins, 13 scenarios: 4 ok · 1 n/a · 2 error · 6 crash
Coverage: 33.3% (4 of the 12 scenarios that apply ran ok)

Crashing  14.2%  1 ok · 6 crash
    crash       ×6  worker killed by signal 11 (SIGSEGV) during PF_Cmd_RENDER
                    size:0, size:1, size:2, size:3, … (+2)

Erring  50.0%  1 ok · 1 error
    error       ×1  Plugin rejected Render with error code: 516
                    render:auto@8

Unloaded  did not load
    error       ×1  Plugin not found: Unloaded.plugin
                    load

Everything ok or n/a (1):
    Clean
";
		assert_eq!(text, expected);
	}

	#[test]
	fn the_rich_report_says_the_same_with_colors_symbols_and_bars() {
		let rich = human(&reports(), false, None, Look::RICH);
		assert!(
			rich.contains('\u{1b}') && rich.contains('✗') && rich.contains('█'),
			"{rich}"
		);
		// Without its escapes, it lays out like the plain report, with a
		// symbol before each status and bars beside the counts.
		let stripped: String = strip_ansi(&rich);
		assert!(stripped.contains("Crashing  ███"), "{stripped}");
		assert!(stripped.contains("    ✗ crash       ×6  worker killed"), "{stripped}");
		assert!(stripped.contains("                      size:0, size:1"), "{stripped}");
		assert!(stripped.contains("✓ Everything ok or n/a (1):"), "{stripped}");
	}

	#[test]
	fn all_lists_every_plugin_and_scenario() {
		let ids: Vec<(String, Status)> = (0..6).map(|i| (format!("size:{i}"), Status::Ok)).collect();
		let ids: Vec<(&str, Status)> = ids.iter().map(|(id, s)| (id.as_str(), *s)).collect();
		let text = human(&[report(&ids)], true, None, Look::PLAIN);
		assert!(
			text.contains(
				"Fx  100.0%  7 ok\n    ok          ×7  load, size:0, size:1, size:2, size:3, size:4, size:5\n"
			),
			"{text}"
		);
		assert!(!text.contains("Everything ok"), "{text}");
	}

	#[test]
	fn coverage_counts_ok_among_the_scenarios_that_apply() {
		use Status::*;
		assert_eq!(coverage(&[Ok, Ok, Partial, Crash, NotApplicable, Invalid]), Some(0.5));
		assert_eq!(coverage(&[NotApplicable]), None);
		// Only every scenario ok reads 100%.
		assert_eq!(percent(1999.0 / 2000.0), "99.9%");
		assert_eq!(percent(1.0), "100.0%");
	}

	#[test]
	fn bars_fill_their_width_and_show_every_status() {
		let counts = BTreeMap::from([(Status::Ok, 4919), (Status::Partial, 1), (Status::Crash, 292)]);
		let plain = bar(Look::PLAIN, &counts, 60);
		assert_eq!(plain.chars().count(), 60);
		let rich = strip_ansi(&bar(Look::RICH, &counts, 60));
		assert_eq!(rich.chars().count(), 60);
		assert_eq!(bar(Look::PLAIN, &BTreeMap::new(), 5), "     ");
	}

	#[test]
	fn wrap_keeps_lines_within_the_width() {
		let items: Vec<String> = (0..40).map(|i| format!("Plugin{i}")).collect();
		let lines = wrap(&items, "    ");
		assert!(
			lines.len() > 1 && lines.iter().all(|l| l.chars().count() <= WIDTH),
			"{lines:?}"
		);
		assert_eq!(lines.join(" ").matches("Plugin").count(), 40);
	}

	/// `text` without its ANSI escapes.
	fn strip_ansi(text: &str) -> String {
		let mut out = String::new();
		let mut chars = text.chars();
		while let Some(c) = chars.next() {
			if c == '\u{1b}' {
				for c in chars.by_ref() {
					if c.is_ascii_alphabetic() {
						break;
					}
				}
			} else {
				out.push(c);
			}
		}
		out
	}
}
