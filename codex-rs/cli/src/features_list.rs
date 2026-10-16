use clap::Args;
use clap::ValueEnum;
use serde::Serialize;
use std::io::Write;

#[derive(Debug, Args)]
pub(crate) struct FeaturesListArgs {
    /// Output format.
    #[arg(long, value_enum, default_value_t = FeaturesListFormat::Table)]
    pub(crate) format: FeaturesListFormat,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum FeaturesListFormat {
    Plain,
    Table,
    Markdown,
    Json,
}

#[derive(Debug, Serialize)]
pub(crate) struct FeaturesListRow<'a> {
    pub(crate) name: &'a str,
    pub(crate) stage: &'a str,
    pub(crate) enabled: bool,
}

#[derive(Serialize)]
struct FeaturesListOutput<'a, 'b> {
    features: &'a [FeaturesListRow<'b>],
}

pub(crate) fn write_features_list(
    mut output: impl Write,
    rows: &[FeaturesListRow<'_>],
    format: FeaturesListFormat,
) -> anyhow::Result<()> {
    let name_width = rows.iter().map(|row| row.name.len()).max().unwrap_or(0);
    let stage_width = rows.iter().map(|row| row.stage.len()).max().unwrap_or(0);

    if matches!(format, FeaturesListFormat::Plain) {
        for row in rows {
            writeln!(
                output,
                "{:<name_width$}  {:<stage_width$}  {}",
                row.name, row.stage, row.enabled
            )?;
        }
        return Ok(());
    }
    if matches!(format, FeaturesListFormat::Json) {
        serde_json::to_writer_pretty(&mut output, &FeaturesListOutput { features: rows })?;
        writeln!(output)?;
        return Ok(());
    }

    // Feature keys and stage labels are fixed ASCII strings from the registry.
    let name_width = name_width.max("name".len());
    let stage_width = stage_width.max("stage".len());
    let enabled_width = "enabled".len();
    let widths = [name_width, stage_width, enabled_width];
    let markdown = matches!(format, FeaturesListFormat::Markdown);
    let vertical = if markdown { '|' } else { '│' };
    if !markdown {
        write_border(&mut output, widths, ['╭', '┬', '╮'], '─')?;
    }
    writeln!(
        output,
        "{vertical} {:<name_width$} {vertical} {:<stage_width$} {vertical} {:<enabled_width$} {vertical}",
        "name", "stage", "enabled"
    )?;
    if markdown {
        write_border(&mut output, widths, ['|', '|', '|'], '-')?;
    } else {
        write_border(&mut output, widths, ['├', '┼', '┤'], '─')?;
    }
    for row in rows {
        writeln!(
            output,
            "{vertical} {:<name_width$} {vertical} {:<stage_width$} {vertical} {:<enabled_width$} {vertical}",
            row.name,
            row.stage,
            row.enabled.to_string()
        )?;
    }
    if !markdown {
        write_border(&mut output, widths, ['╰', '┴', '╯'], '─')?;
    }
    Ok(())
}

fn write_border(
    output: &mut impl Write,
    widths: [usize; 3],
    edges: [char; 3],
    horizontal: char,
) -> std::io::Result<()> {
    writeln!(
        output,
        "{}{}{}{}{}{}{}",
        edges[0],
        horizontal.to_string().repeat(widths[0] + 2),
        edges[1],
        horizontal.to_string().repeat(widths[1] + 2),
        edges[1],
        horizontal.to_string().repeat(widths[2] + 2),
        edges[2]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn render(rows: &[FeaturesListRow<'_>], format: FeaturesListFormat) -> String {
        let mut output = Vec::new();
        write_features_list(&mut output, rows, format).unwrap();
        String::from_utf8(output).unwrap()
    }

    fn rows() -> [FeaturesListRow<'static>; 2] {
        [
            FeaturesListRow {
                name: "long_feature",
                stage: "under development",
                enabled: false,
            },
            FeaturesListRow {
                name: "undo",
                stage: "stable",
                enabled: true,
            },
        ]
    }

    #[test]
    fn plain_preserves_aligned_output() {
        assert_eq!(
            render(&rows(), FeaturesListFormat::Plain),
            "long_feature  under development  false\nundo          stable             true\n"
        );
        assert_eq!(render(&[], FeaturesListFormat::Plain), "");
    }

    #[test]
    fn table_has_headers_and_aligned_columns() {
        assert_eq!(
            render(&rows(), FeaturesListFormat::Table),
            concat!(
                "╭──────────────┬───────────────────┬─────────╮\n",
                "│ name         │ stage             │ enabled │\n",
                "├──────────────┼───────────────────┼─────────┤\n",
                "│ long_feature │ under development │ false   │\n",
                "│ undo         │ stable            │ true    │\n",
                "╰──────────────┴───────────────────┴─────────╯\n",
            )
        );
    }

    #[test]
    fn markdown_has_headers_and_separator() {
        assert_eq!(
            render(&rows(), FeaturesListFormat::Markdown),
            concat!(
                "| name         | stage             | enabled |\n",
                "|--------------|-------------------|---------|\n",
                "| long_feature | under development | false   |\n",
                "| undo         | stable            | true    |\n",
            )
        );
    }

    #[test]
    fn empty_table_sizes_columns_for_headers() {
        assert_eq!(
            render(&[], FeaturesListFormat::Table),
            concat!(
                "╭──────┬───────┬─────────╮\n",
                "│ name │ stage │ enabled │\n",
                "├──────┼───────┼─────────┤\n",
                "╰──────┴───────┴─────────╯\n",
            )
        );
        assert_eq!(
            render(&[], FeaturesListFormat::Markdown),
            "| name | stage | enabled |\n|------|-------|---------|\n"
        );
    }

    #[test]
    fn json_uses_a_features_array_and_boolean_states() {
        let output = render(&rows(), FeaturesListFormat::Json);
        assert!(output.ends_with('\n'));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).unwrap(),
            serde_json::json!({"features": [
                {"name": "long_feature", "stage": "under development", "enabled": false},
                {"name": "undo", "stage": "stable", "enabled": true},
            ]})
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&render(&[], FeaturesListFormat::Json))
                .unwrap(),
            serde_json::json!({"features": []})
        );
    }

    #[test]
    fn output_errors_are_propagated() {
        for format in [
            FeaturesListFormat::Plain,
            FeaturesListFormat::Table,
            FeaturesListFormat::Markdown,
            FeaturesListFormat::Json,
        ] {
            assert!(write_features_list(&mut [][..], &rows(), format).is_err());
        }
    }
}
