//! Country holiday packs embedded at build time (FR-034, research R21).
use chrono::NaiveDate;
use serde::Serialize;

macro_rules! pack {
    ($id:literal, $name:literal, $file:literal) => {
        (
            $id,
            $name,
            include_str!(concat!("../../../holidays/", $file)),
        )
    };
}

/// `(id, display name, CSV)`; keep in sync with `holidays/` (a test checks every file is listed).
const PACKS: &[(&str, &str, &str)] = &[
    pack!("IN/2026", "India — national (2026)", "IN/2026.csv"),
    pack!("IN/2027", "India — national (2027)", "IN/2027.csv"),
];

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct PackInfo {
    /// e.g. `IN/2027`
    pub id: &'static str,
    pub name: &'static str,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackHoliday {
    pub date: NaiveDate,
    pub name: String,
    pub kind: String,
}

pub fn list() -> Vec<PackInfo> {
    PACKS
        .iter()
        .map(|(id, name, csv)| PackInfo {
            id,
            name,
            count: parse(csv).len(),
        })
        .collect()
}

pub fn get(id: &str) -> Option<Vec<PackHoliday>> {
    PACKS.iter().find(|p| p.0 == id).map(|p| parse(p.2))
}

/// Packs are validated in CI, so malformed lines are simply skipped here.
fn parse(csv: &str) -> Vec<PackHoliday> {
    csv.lines()
        .skip(1)
        .filter_map(|line| {
            let mut cols = line.splitn(3, ',').map(str::trim);
            let date = cols.next()?.parse().ok()?;
            let name = cols.next()?.to_string();
            let kind = cols.next()?.to_string();
            Some(PackHoliday { date, name, kind })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pack_file_on_disk_is_embedded() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../holidays");
        let mut on_disk = Vec::new();
        for country in std::fs::read_dir(&root).unwrap() {
            let country = country.unwrap().path();
            if !country.is_dir() {
                continue;
            }
            for f in std::fs::read_dir(&country).unwrap() {
                let f = f.unwrap().path();
                if f.extension().is_some_and(|e| e == "csv") {
                    on_disk.push(format!(
                        "{}/{}",
                        country.file_name().unwrap().to_string_lossy(),
                        f.file_stem().unwrap().to_string_lossy()
                    ));
                }
            }
        }
        on_disk.sort();
        let mut embedded: Vec<String> = PACKS.iter().map(|p| p.0.to_string()).collect();
        embedded.sort();
        assert_eq!(on_disk, embedded);
    }

    #[test]
    fn india_packs_have_the_17_gazetted_holidays() {
        for id in ["IN/2026", "IN/2027"] {
            let h = get(id).unwrap();
            assert_eq!(h.len(), 17, "{id}");
            assert!(
                h.iter()
                    .any(|d| d.name == "Republic Day"
                        && d.date.format("%m-%d").to_string() == "01-26")
            );
        }
    }
}
