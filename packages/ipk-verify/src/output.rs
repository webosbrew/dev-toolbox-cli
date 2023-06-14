use std::fs::File;
use std::io::{Error, Stdout, Write};

use prettytable::format::{FormatBuilder, LinePosition, LineSeparator, TableFormat};
use prettytable::{Cell, Table};
use term::{color, Attr};

use verify_lib::ipk::ComponentBinVerifyResult;

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
