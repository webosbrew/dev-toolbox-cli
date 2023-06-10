use std::fs::File;
use std::io::{Error, Stdout, Write};

use crate::OutputFormat;
use common::{BinVerifyResult, VerifyResult};
use prettytable::format::{FormatBuilder, LinePosition, LineSeparator, TableFormat};
use prettytable::{Cell, Table};
use term::{color, Attr};

pub trait PrintTable {
    fn result_cell(&self, result: &BinVerifyResult, out_fmt: &OutputFormat) -> Cell {
        return if result.is_good() {
            let mut cell = Cell::new(if *out_fmt == OutputFormat::Markdown {
                ":ok:"
            } else {
                "OK"
            });
            cell.style(Attr::ForegroundColor(color::BRIGHT_GREEN));
            cell
        } else {
            let mut cell = Cell::new(if *out_fmt == OutputFormat::Markdown {
                ":x:"
            } else {
                "FAIL"
            });
            cell.style(Attr::ForegroundColor(color::BRIGHT_RED));
            cell
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

    fn print_details(&mut self, );
}

pub trait ReportOutput: PrintTable + Write {}

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
