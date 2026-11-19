//! `LEDGER_DEMO=path`: writes a small set of books there and opens it, so the
//! screenshots have something in them.

use std::path::Path;

use crate::store::{self, Write};

pub fn make(path: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(path);
    let doc = store::create(path)?;
    let w = &doc.writer;
    let cell = |sheet: i64, row: u32, col: u32, input: &str| {
        w.send(Write::Cell { sheet, pos: (row, col), input: input.into() });
    };
    w.send(Write::RenameSheet { id: 1, name: "Income".into() });
    for (c, h) in ["Date", "Client", "Invoice", "Amount", "VAT 25%", "Total"].iter().enumerate() {
        cell(1, 0, c as u32, h);
    }
    let rows = [
        ("2026-01-12", "Northwind AB", "INV-001", "12000"),
        ("2026-01-30", "Fjord Labs", "INV-002", "8500"),
        ("2026-02-14", "Northwind AB", "INV-003", "12000"),
        ("2026-03-02", "Polar Studio", "INV-004", "4300.50"),
        ("2026-03-21", "Fjord Labs", "INV-005", "9100"),
    ];
    for (i, (date, client, inv, amount)) in rows.iter().enumerate() {
        let r = i as u32 + 1;
        cell(1, r, 0, date);
        cell(1, r, 1, client);
        cell(1, r, 2, inv);
        cell(1, r, 3, amount);
        cell(1, r, 4, &format!("=ROUND(D{0}*0.25, 2)", r + 1));
        cell(1, r, 5, &format!("=D{0}+E{0}", r + 1));
    }
    cell(1, 7, 1, "Total");
    for c in [3, 4, 5] {
        let col = crate::formula::col_name(c);
        cell(1, 7, c, &format!("=SUM({col}2:{col}6)"));
    }
    w.send(Write::AddSheet { id: 2, name: "Expenses".into() });
    for (c, h) in ["Date", "Paid to", "Category", "Amount"].iter().enumerate() {
        cell(2, 0, c as u32, h);
    }
    let rows = [
        ("2026-01-03", "Studio rent", "Rent", "3200"),
        ("2026-01-15", "Fiber", "Internet", "399"),
        ("2026-02-03", "Studio rent", "Rent", "3200"),
        ("2026-02-20", "Laptop", "Equipment", "18900"),
    ];
    for (i, (date, to, cat, amount)) in rows.iter().enumerate() {
        let r = i as u32 + 1;
        cell(2, r, 0, date);
        cell(2, r, 1, to);
        cell(2, r, 2, cat);
        cell(2, r, 3, amount);
    }
    cell(2, 6, 2, "Total");
    cell(2, 6, 3, "=SUM(D2:D5)");
    // A sheet of shell output, stored as if the command had run, so the demo
    // never runs anything. The block is smaller than the output on both
    // edges, so the "…" marks show.
    w.send(Write::AddSheet { id: 3, name: "Disk".into() });
    let formula = "=SH(\"df -h\", :E6, rest=drop)";
    cell(3, 0, 0, formula);
    let stdout = "Filesystem Size Used Avail Use% Mounted\n\
        /dev/nvme0n1p2 916G 412G 458G 48% /\n\
        /dev/nvme0n1p1 1.0G 6.2M 1.0G 1% /boot/efi\n\
        /dev/sda1 3.6T 2.1T 1.4T 61% /mnt/backup\n\
        tmpfs 7.8G 12M 7.8G 1% /run/user/1000\n\
        /dev/sdb1 1.8T 910G 820G 53% /mnt/media\n\
        /dev/mapper/vault 500G 120G 355G 26% /mnt/vault\n\
        /dev/sdc1 2.7T 1.9T 700G 74% /mnt/scratch\n";
    w.send(Write::Shell {
        sheet: 3,
        pos: (0, 0),
        run: crate::shell::Run {
            formula: formula.into(),
            status: crate::shell::Status::Ok,
            stdout: stdout.into(),
            stderr: String::new(),
            ran_at: 1_790_000_000,
        },
    });
    cell(3, 9, 0, "Output of df -h, cut off at row 6 and column E");
    Ok(())
}
