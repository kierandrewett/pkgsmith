use msi::{Column, Expr, Insert, Package, PackageType, Select, Update, Value};
use std::io::Cursor;

// ========================================================================= //

const ROWS: usize = 30_000;

fn new_package() -> Package<Cursor<Vec<u8>>> {
    let cursor = Cursor::new(Vec::new());
    let mut package = Package::create(PackageType::Installer, cursor).unwrap();
    for table in ["Alpha", "Beta"] {
        package
            .create_table(
                table,
                vec![
                    Column::build("Key").primary_key().string(72),
                    Column::build("Name").string(72),
                    Column::build("Number").nullable().int32(),
                ],
            )
            .unwrap();
    }
    package
}

fn rows(table: &str, range: std::ops::Range<usize>) -> Insert {
    Insert::into(table).rows(
        range
            .map(|n| {
                vec![
                    Value::Str(format!("{table}-key-{n}")),
                    Value::Str(format!("{table}-name-{n}")),
                    Value::Int(n as i32),
                ]
            })
            .collect(),
    )
}

fn count(package: &mut Package<Cursor<Vec<u8>>>, table: &str) -> usize {
    package.select_rows(Select::table(table)).unwrap().count()
}

// ========================================================================= //

#[test]
fn small_database_keeps_short_string_refs() {
    let mut package = new_package();
    package.insert_rows(rows("Alpha", 0..100)).unwrap();
    assert!(!package.long_string_refs());
    let cursor = package.into_inner().unwrap();
    let mut package = Package::open(cursor).unwrap();
    assert!(!package.long_string_refs());
    assert_eq!(count(&mut package, "Alpha"), 100);
}

#[test]
fn many_strings_in_one_insert() {
    let mut package = new_package();
    package.insert_rows(rows("Alpha", 0..ROWS)).unwrap();
    package.insert_rows(rows("Beta", 0..ROWS)).unwrap();
    assert!(package.long_string_refs());
    assert_eq!(count(&mut package, "Alpha"), ROWS);

    let cursor = package.into_inner().unwrap();
    let mut package = Package::open(cursor).unwrap();
    assert!(package.long_string_refs());
    assert!(package.has_table("Alpha"));
    assert!(package.has_table("Beta"));
    assert_eq!(count(&mut package, "Alpha"), ROWS);
    assert_eq!(count(&mut package, "Beta"), ROWS);
    let wanted = Select::table("Beta")
        .with(Expr::col("Key").eq(Expr::string("Beta-key-29999")));
    let found: Vec<_> = package.select_rows(wanted).unwrap().collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["Name"], Value::Str("Beta-name-29999".to_string()));
    assert_eq!(found[0]["Number"], Value::Int(29_999));
}

#[test]
fn switch_rewrites_rows_that_were_already_there() {
    let mut package = new_package();
    // 2 * 30,000 = 60,000 strings fit in two-byte refs, and the tables hold
    // rows when the next insert needs more.
    package.insert_rows(rows("Alpha", 0..ROWS)).unwrap();
    assert!(!package.long_string_refs());
    package.insert_rows(rows("Alpha", ROWS..ROWS + 100)).unwrap();
    assert!(!package.long_string_refs());
    package.insert_rows(rows("Beta", 0..ROWS)).unwrap();
    assert!(package.long_string_refs());

    let cursor = package.into_inner().unwrap();
    let mut package = Package::open(cursor).unwrap();
    assert!(package.long_string_refs());
    assert_eq!(count(&mut package, "Alpha"), ROWS + 100);
    assert_eq!(count(&mut package, "Beta"), ROWS);
    let wanted = Select::table("Alpha")
        .with(Expr::col("Key").eq(Expr::string("Alpha-key-0")));
    let found: Vec<_> = package.select_rows(wanted).unwrap().collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["Name"], Value::Str("Alpha-name-0".to_string()));
    // The columns of every table, including the ones the crate keeps itself,
    // still read back.
    let tables: Vec<String> =
        package.tables().map(|table| table.name().to_string()).collect();
    assert_eq!(tables.len(), 5);
    let alpha = package.get_table("Alpha").unwrap();
    assert_eq!(alpha.columns().len(), 3);
    assert!(alpha.columns()[0].is_primary_key());
}

#[test]
fn update_after_switch() {
    let mut package = new_package();
    package.insert_rows(rows("Alpha", 0..ROWS)).unwrap();
    package.insert_rows(rows("Beta", 0..ROWS)).unwrap();
    assert!(package.long_string_refs());
    package
        .update_rows(
            Update::table("Beta")
                .set("Name", Value::Str("renamed".to_string()))
                .with(Expr::col("Number").eq(Expr::integer(7))),
        )
        .unwrap();
    let cursor = package.into_inner().unwrap();
    let mut package = Package::open(cursor).unwrap();
    let wanted = Select::table("Beta")
        .with(Expr::col("Name").eq(Expr::string("renamed")));
    let found: Vec<_> = package.select_rows(wanted).unwrap().collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["Key"], Value::Str("Beta-key-7".to_string()));
}

#[test]
fn update_switches_to_long_string_refs() {
    let mut package = new_package();
    // One string per row, and a shared name in a small table: the pool is
    // nearly full, but two bytes still address it.
    package
        .insert_rows(Insert::into("Beta").rows(
            (0..63_000)
                .map(|n| {
                    vec![
                        Value::Str(format!("key-{n}")),
                        Value::Str("shared".to_string()),
                        Value::Int(n),
                    ]
                })
                .collect(),
        ))
        .unwrap();
    package
        .insert_rows(Insert::into("Alpha").rows(
            (0..2_000)
                .map(|n| {
                    vec![
                        Value::Str(format!("small-{n}")),
                        Value::Str("shared".to_string()),
                        Value::Int(n),
                    ]
                })
                .collect(),
        ))
        .unwrap();
    assert!(!package.long_string_refs());
    // Each update adds one string, until the pool is full and has to switch.
    let mut updates = 0;
    while !package.long_string_refs() {
        assert!(updates < 2_000, "the pool never switched");
        package
            .update_rows(
                Update::table("Alpha")
                    .set("Name", Value::Str(format!("update-{updates}")))
                    .with(Expr::col("Number").eq(Expr::integer(updates))),
            )
            .unwrap();
        updates += 1;
    }
    assert!(updates > 1);
    let cursor = package.into_inner().unwrap();
    let mut package = Package::open(cursor).unwrap();
    assert!(package.long_string_refs());
    assert_eq!(count(&mut package, "Beta"), 63_000);
    assert_eq!(count(&mut package, "Alpha"), 2_000);
    let wanted = Select::table("Alpha")
        .with(Expr::col("Name").eq(Expr::string(format!("update-{}", updates - 1))));
    let found: Vec<_> = package.select_rows(wanted).unwrap().collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["Number"], Value::Int(updates - 1));
}
