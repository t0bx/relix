/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::ColumnInfo;

const AUTO_RANDOM: &str = "AUTO_RANDOM";

/// Marks TiDB `AUTO_RANDOM` columns as generated. TiDB leaves the attribute out of
/// `information_schema.COLUMNS.EXTRA`; it only appears in the column clause of
/// `SHOW CREATE TABLE` as `/*T![auto_rand] AUTO_RANDOM(5) */`.
pub fn mark_auto_random(columns: &mut [ColumnInfo], definition: &str) {
    for clause in definition.lines().map(str::trim_start) {
        if !clause.contains(AUTO_RANDOM) {
            continue;
        }
        let Some(name) = leading_identifier(clause) else {
            continue;
        };
        if let Some(column) = columns.iter_mut().find(|column| column.name == name) {
            column.is_auto_increment = true;
        }
    }
}

fn leading_identifier(clause: &str) -> Option<String> {
    let mut chars = clause.strip_prefix('`')?.chars().peekable();
    let mut name = String::new();
    while let Some(ch) = chars.next() {
        if ch != '`' {
            name.push(ch);
        } else if chars.peek() == Some(&'`') {
            chars.next();
            name.push('`');
        } else {
            return Some(name);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            data_type: "bigint(20)".into(),
            nullable: false,
            default_value: None,
            ordinal: 1,
            is_primary_key: false,
            is_auto_increment: false,
            comment: None,
        }
    }

    #[test]
    fn auto_random_columns_are_marked_from_the_ddl() {
        let mut columns = vec![column("id"), column("we`ird"), column("v")];
        let ddl = "CREATE TABLE `ar` (\n  `id` bigint(20) NOT NULL /*T![auto_rand] AUTO_RANDOM(5) */,\n  \
                   `we``ird` bigint(20) /*T![auto_rand] AUTO_RANDOM(3) */,\n  `v` int(11) DEFAULT NULL,\n  \
                   PRIMARY KEY (`id`) /*T![clustered_index] CLUSTERED */\n)";
        mark_auto_random(&mut columns, ddl);
        assert!(columns[0].is_auto_increment);
        assert!(columns[1].is_auto_increment);
        assert!(!columns[2].is_auto_increment);
    }

    #[test]
    fn unterminated_identifiers_are_ignored() {
        assert_eq!(leading_identifier("`open AUTO_RANDOM"), None);
        assert_eq!(leading_identifier("PRIMARY KEY"), None);
    }
}
