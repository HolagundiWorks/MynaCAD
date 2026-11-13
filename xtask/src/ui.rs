//! `cargo xtask ui`: keeps the WinUI shell on its design system (docs/UI_STANDARDS.md).
//!
//! Two checks over `apps/aad-winui`, both pure text scans so they run on every platform:
//!
//! 1. **Window template.** A file that creates a top-level window must open it through
//!    `StandardWindow.Present` (chrome, owner, sizing, keyboard). Calling `ShellIcon.Apply` or
//!    `ShellFit.*` directly is reserved for the design-system files and the listed exceptions.
//! 2. **No new hard-coded dimensions.** Margins, paddings, spacings, sizes, font sizes and corner
//!    radii written as numbers instead of tokens are counted per file. A file may not exceed its
//!    recorded count; new files start at zero. Lower a count here when a file is cleaned up.

use std::path::Path;

/// Files that define the tokens and components; they are expected to contain numbers.
const TOKEN_SOURCES: &[&str] = &["Density.cs", "Standard.cs", "ShellFit.cs", "ToolIcons.cs", "ArchIcons.cs"];

/// Windows that may set themselves up without `StandardWindow.Present`, and why.
const WINDOW_EXCEPTIONS: &[(&str, &str)] = &[
    ("App.xaml.cs", "creates the main shell window"),
    ("MainWindow.xaml.cs", "main shell; its PDF, Panels and Clear history windows move to templates in a follow-up"),
    ("CanvasMenu.cs", "borderless context popup placed at the cursor"),
    ("CommandChrome.cs", "borderless command overlays over the drawing"),
    ("SmartBlockWindow.cs", "arrives with the smart-blocks change; move it to StandardWindow.Present after that lands"),
];

/// Highest allowed count of hard-coded dimensions per file (ratchet; only ever lower these).
/// `MainWindow.xaml.cs` and `SmartBlockWindow.cs` include the smart-blocks change still in review.
const LITERAL_BASELINE: &[(&str, usize)] = &[
    ("BatchPlotWindow.cs", 2),
    ("CanvasMenu.cs", 4),
    ("CommandChrome.cs", 2),
    ("DimStyleWindow.cs", 5),
    ("DraftingWindow.cs", 1),
    ("HatchWindow.cs", 2),
    ("LevelBars.cs", 2),
    ("LinetypeWindow.cs", 5),
    ("MLeaderStyleWindow.cs", 4),
    ("MainWindow.xaml.cs", 24),
    ("PageSetupWindow.cs", 5),
    ("ParametersWindow.cs", 5),
    ("PlotStyleWindow.cs", 7),
    ("PointStyleWindow.cs", 4),
    ("PomodoroWindow.cs", 1),
    ("QuickSelectWindow.cs", 6),
    ("QuickWindow.cs", 5),
    ("RoomSettingsWindow.cs", 4),
    ("SmartBlockWindow.cs", 4),
    ("TextStyleWindow.cs", 4),
    ("UnitsWindow.cs", 3),
];

const SIZE_PROPS: &[&str] = &[
    "Margin",
    "Padding",
    "Spacing",
    "RowSpacing",
    "ColumnSpacing",
    "Width",
    "Height",
    "MinWidth",
    "MinHeight",
    "MaxWidth",
    "MaxHeight",
    "FontSize",
    "CornerRadius",
];

const RAW_WINDOW_SETUP: &[&str] = &["ShellIcon.Apply(", "ShellFit.ToContent(", "ShellFit.UserResizable(", "ShellFit.Adaptive(", "ShellFit.Attach("];

fn starts_nonzero_number(rest: &str) -> bool {
    let rest = rest.trim_start();
    matches!(rest.as_bytes().first(), Some(b'1'..=b'9'))
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Count `needle` occurrences in `line` followed by a non-zero number, with an identifier
/// boundary before the needle.
fn count_followed_by_number(line: &str, needle: &str) -> usize {
    let mut count = 0;
    let mut from = 0;
    while let Some(pos) = line.get(from..).and_then(|s| s.find(needle)) {
        let at = from + pos;
        let boundary = at == 0 || line.as_bytes().get(at - 1).is_none_or(|b| !is_ident(*b));
        let after = at + needle.len();
        if boundary && line.get(after..).is_some_and(starts_nonzero_number) {
            count += 1;
        }
        from = after;
    }
    count
}

/// Hard-coded dimensions in one C# or XAML source.
pub fn count_literals(source: &str, xaml: bool) -> usize {
    let mut total = 0;
    for raw in source.lines() {
        let line = raw.trim_start();
        if line.starts_with("//") || line.starts_with("///") || line.starts_with("<!--") {
            continue;
        }
        if xaml {
            for prop in SIZE_PROPS {
                total += count_followed_by_number(line, &format!("{prop}=\""));
            }
        } else {
            for prop in SIZE_PROPS {
                total += count_followed_by_number(line, &format!("{prop} = "));
            }
            total += count_followed_by_number(line, "new Thickness(");
            total += count_followed_by_number(line, "new CornerRadius(");
        }
    }
    total
}

/// Does this C# source create a top-level window (`new Window ...` or a `: Window` class)?
pub fn creates_window(source: &str) -> bool {
    source.lines().any(|raw| {
        let line = raw.trim();
        if line.starts_with("//") {
            return false;
        }
        let constructs = line.match_indices("new Window").any(|(at, needle)| {
            let next = line.as_bytes().get(at + needle.len()).copied();
            next.is_none_or(|b| !is_ident(b) && b != b'?')
        });
        constructs || line.ends_with(": Window")
    })
}

/// Buttons that are passed to two `ActionRow(` / `Footer(` calls within 80 lines of each other. A XAML element has
/// one parent, so the second call throws; the Map Layers wizard crashed the shell that way (one Cancel in two rows).
pub fn shared_action_buttons(source: &str) -> Vec<String> {
    let mut seen: Vec<(usize, Vec<String>)> = Vec::new();
    let mut found = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let code = line.trim();
        if code.starts_with("//") {
            continue;
        }
        let Some(at) = code.find("ActionRow(").or_else(|| code.find("Footer(")) else {
            continue;
        };
        let args = &code[at..];
        let idents: Vec<String> = args
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|w| w.chars().next().is_some_and(|c| c.is_ascii_lowercase()) && !w.contains("ActionRow"))
            .map(str::to_string)
            .collect();
        for (earlier, names) in &seen {
            if index.saturating_sub(*earlier) > 80 {
                continue;
            }
            for name in &idents {
                if names.contains(name) && !found.contains(name) {
                    found.push(name.clone());
                }
            }
        }
        seen.push((index, idents));
    }
    found
}

/// Problems with one file, given its name and contents.
pub fn check_file(name: &str, source: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let xaml = name.ends_with(".xaml");
    let exception = WINDOW_EXCEPTIONS.iter().any(|(file, _)| *file == name);
    if !xaml && !TOKEN_SOURCES.contains(&name) && name != "ShellIcon.cs" && !exception {
        if creates_window(source) && !source.contains("StandardWindow.Present(") {
            problems.push(format!("{name}: creates a window without StandardWindow.Present (pick a WindowTemplate)"));
        }
        for call in RAW_WINDOW_SETUP {
            if source.contains(call) {
                problems.push(format!("{name}: calls {call}..) directly; use StandardWindow.Present"));
            }
        }
    }
    for button in shared_action_buttons(source) {
        problems.push(format!("{name}: `{button}` is added to two action rows; a XAML element has one parent, make one per row"));
    }
    if !TOKEN_SOURCES.contains(&name) && !name.starts_with("Themes/") {
        let found = count_literals(source, xaml);
        let allowed = LITERAL_BASELINE.iter().find(|(file, _)| *file == name).map_or(0, |(_, n)| *n);
        if found > allowed {
            problems.push(format!("{name}: {found} hard-coded dimension(s), {allowed} allowed; use Density / Themes tokens or Ui components"));
        }
    }
    problems
}

pub fn run(root: &Path, show_counts: bool) -> Result<(), String> {
    let dir = root.join("apps/aad-winui");
    let mut names = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".cs") || name.ends_with(".xaml") {
            names.push(name);
        }
    }
    names.sort();
    let mut problems = Vec::new();
    for name in &names {
        let bytes = std::fs::read(dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
        let source = String::from_utf8_lossy(&bytes);
        if show_counts && !TOKEN_SOURCES.contains(&name.as_str()) {
            println!("{name}: {}", count_literals(&source, name.ends_with(".xaml")));
        }
        problems.extend(check_file(name, &source));
    }
    if problems.is_empty() {
        println!("ui: {} WinUI sources follow the window templates and token rules", names.len());
        Ok(())
    } else {
        Err(format!("{} UI design-system problem(s) (docs/UI_STANDARDS.md):\n  {}", problems.len(), problems.join("\n  ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_numbers_but_not_tokens() {
        let cs = "var a = new StackPanel { Spacing = 6, Margin = Density.DialogMargin };\n\
                  var b = new Border { Padding = new Thickness(4, 0, 4, 0), BorderThickness = new Thickness(0) };\n\
                  // Width = 12 in a comment\n\
                  box.MinWidth = 80; box.FontSize = Density.FontBody; x.LineWidth = 3;";
        assert_eq!(count_literals(cs, false), 3);
        let xaml = r#"<Grid Margin="4,2" Padding="{StaticResource PaddingPanel}" Width="1" BorderThickness="0,0,0,1" Height="0" />"#;
        assert_eq!(count_literals(xaml, true), 2);
    }

    #[test]
    fn new_window_needs_the_template() {
        let raw = "var window = new Window { Title = \"X\" };\nShellIcon.Apply(window, theme);";
        let problems = check_file("NewToolWindow.cs", raw);
        assert_eq!(problems.len(), 2, "{problems:?}");
        let good = "var window = new Window { Title = \"X\" };\nStandardWindow.Present(window, owner, WindowTemplate.Dialog);";
        assert!(check_file("NewToolWindow.cs", good).is_empty());
        let class = "internal sealed class FooWindow : Window\n{\n}";
        assert_eq!(check_file("FooWindow.cs", class).len(), 1);
    }

    #[test]
    fn one_button_in_two_action_rows_is_refused() {
        let bad = "pick.Children.Add(DialogLayout.ActionRow(review, cancel));
check.Children.Add(DialogLayout.ActionRow(apply, back, cancel));";
        assert_eq!(shared_action_buttons(bad), vec!["cancel".to_string()]);
        let good = "pick.Children.Add(DialogLayout.ActionRow(review, cancel));
check.Children.Add(DialogLayout.ActionRow(apply, back, cancelReview));";
        assert!(shared_action_buttons(good).is_empty());
    }

    #[test]
    fn new_files_get_no_literal_allowance() {
        let problems = check_file("Fresh.cs", "var s = new StackPanel { Spacing = 6 };");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(check_file("Density.cs", "public const double SpaceMd = 6; x.Spacing = 6;").is_empty());
    }
}
