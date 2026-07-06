use std::fs::File;
use std::io::{Error, Stdout, Write};

use prettytable::format::{FormatBuilder, LinePosition, LineSeparator, TableFormat};
use prettytable::{Cell, Table};
use term::{color, Attr};

use verify_lib::ipk::{ComponentBinVerifyResult, CompatVerdict};

use crate::OutputFormat;

pub trait PrintTable {
    fn result_cell(&self, result: &ComponentBinVerifyResult, out_fmt: &OutputFormat) -> Cell {
        return match result {
            ComponentBinVerifyResult::Ok { .. } => {
                let mut cell = Cell::new(if *out_fmt == OutputFormat::Markdown {
                    ":ok:"
                } else {
                    "OK"
                });
                cell.style(Attr::ForegroundColor(color::BRIGHT_GREEN));
                cell
            }
            ComponentBinVerifyResult::Skipped { .. } => Cell::new("SKIP"),
            ComponentBinVerifyResult::Failed(_) => {
                let mut cell = Cell::new(if *out_fmt == OutputFormat::Markdown {
                    ":x:"
                } else {
                    "FAIL"
                });
                cell.style(Attr::ForegroundColor(color::BRIGHT_RED));
                cell
            }
        };
    }

    /// Cell for a non-native compatibility verdict, styled like `result_cell`.
    fn verdict_cell(&self, verdict: &CompatVerdict, out_fmt: &OutputFormat) -> Cell {
        return match verdict {
            CompatVerdict::Ok => {
                let mut cell = Cell::new(if *out_fmt == OutputFormat::Markdown {
                    ":ok:"
                } else {
                    "OK"
                });
                cell.style(Attr::ForegroundColor(color::BRIGHT_GREEN));
                cell
            }
            CompatVerdict::Fail { .. } => {
                let mut cell = Cell::new(if *out_fmt == OutputFormat::Markdown {
                    ":x:"
                } else {
                    "FAIL"
                });
                cell.style(Attr::ForegroundColor(color::BRIGHT_RED));
                cell
            }
            CompatVerdict::Unknown => Cell::new(if *out_fmt == OutputFormat::Markdown {
                ":grey_question:"
            } else {
                "UNKNOWN"
            }),
        };
    }

    /// Cell for an advisory (non-gating) runtime-API verdict: native support vs
    /// "may need a polyfill". Rendered in a softer style than a hard FAIL.
    fn advisory_cell(&self, verdict: &CompatVerdict, out_fmt: &OutputFormat) -> Cell {
        return match verdict {
            CompatVerdict::Ok => {
                let mut cell = Cell::new(if *out_fmt == OutputFormat::Markdown {
                    ":ok:"
                } else {
                    "native"
                });
                cell.style(Attr::ForegroundColor(color::BRIGHT_GREEN));
                cell
            }
            CompatVerdict::Fail { .. } => {
                let mut cell = Cell::new(if *out_fmt == OutputFormat::Markdown {
                    ":warning:"
                } else {
                    "polyfill?"
                });
                cell.style(Attr::ForegroundColor(color::YELLOW));
                cell
            }
            CompatVerdict::Unknown => Cell::new("—"),
        };
    }

    fn table_format(&self, out_fmt: &OutputFormat) -> TableFormat {
        match out_fmt {
            OutputFormat::Markdown => FormatBuilder::new()
                .column_separator('|')
                .borders('|')
                .padding(1, 1)
                .separator(LinePosition::Title, LineSeparator::new('-', '|', '|', '|'))
                .build(),
            OutputFormat::Terminal => *prettytable::format::consts::FORMAT_BOX_CHARS,
            OutputFormat::Plain => *prettytable::format::consts::FORMAT_DEFAULT,
        }
    }

    fn print_table(&mut self, table: &Table) -> Result<(), Error>;
}

pub trait ReportOutput: PrintTable + Write {
    fn h2(&mut self, heading: &str) -> Result<(), Error> {
        return self.write_fmt(format_args!("## {heading}\n\n"));
    }

    fn h3(&mut self, heading: &str) -> Result<(), Error> {
        return self.write_fmt(format_args!("### {heading}\n\n"));
    }

    fn h4(&mut self, heading: &str) -> Result<(), Error> {
        return self.write_fmt(format_args!("#### {heading}\n\n"));
    }

    fn h5(&mut self, heading: &str) -> Result<(), Error> {
        return self.write_fmt(format_args!("##### {heading}\n\n"));
    }
}

impl PrintTable for Stdout {
    fn print_table(&mut self, table: &Table) -> Result<(), Error> {
        table.print_tty(false)?;
        println!("\n");
        return Ok(());
    }
}

impl PrintTable for File {
    fn print_table(&mut self, table: &Table) -> Result<(), Error> {
        table.print(self)?;
        self.write_all(b"\n")?;
        return Ok(());
    }
}

impl ReportOutput for Stdout {}

impl ReportOutput for File {}
