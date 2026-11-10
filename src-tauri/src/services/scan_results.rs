//! Immutable scan data with lazy sorting indexes and page-sized cloning.
use crate::{
    error::{AppError, AppResult},
    models::{ScanPage, ScanQuery, ScanSnapshot, SortDirection, SortField},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub struct CachedScan {
    pub snapshot: ScanSnapshot,
    totals: (u64, u64, u64),
    orders: Mutex<HashMap<SortField, Arc<Vec<usize>>>>,
}

impl CachedScan {
    pub fn new(snapshot: ScanSnapshot) -> Self {
        let totals = (
            snapshot.total_bytes(),
            snapshot.total_allocated_bytes(),
            snapshot.total_files(),
        );
        Self {
            snapshot,
            totals,
            orders: Mutex::new(HashMap::new()),
        }
    }

    fn order(&self, field: SortField) -> AppResult<Arc<Vec<usize>>> {
        let mut orders = self
            .orders
            .lock()
            .map_err(|_| AppError::Task("扫描排序缓存锁已损坏。".to_string()))?;
        if let Some(order) = orders.get(&field) {
            return Ok(Arc::clone(order));
        }
        let rows = &self.snapshot.folders;
        let mut order: Vec<_> = (0..rows.len()).collect();
        match field {
            SortField::Name => order.sort_by_cached_key(|&index| rows[index].name.to_lowercase()),
            SortField::Bytes => order.sort_by_key(|&index| rows[index].bytes),
            SortField::AllocatedBytes => order.sort_by_key(|&index| rows[index].allocated_bytes),
            SortField::FileCount => order.sort_by_key(|&index| rows[index].file_count),
            SortField::Status => {
                order.sort_by(|&left, &right| rows[left].note.cmp(&rows[right].note))
            }
        }
        let order = Arc::new(order);
        orders.insert(field, Arc::clone(&order));
        Ok(order)
    }

    pub fn page(&self, query: ScanQuery) -> AppResult<ScanPage> {
        let page_size = query.page_size.clamp(20, 500);
        let total_items = self.snapshot.folders.len();
        let page = query
            .page
            .max(1)
            .min(total_items.div_ceil(page_size).max(1));
        let start = (page - 1) * page_size;
        let order = self.order(query.sort_field)?;
        let row_at = |position: usize| {
            let offset = if matches!(query.sort_direction, SortDirection::Descending) {
                total_items - 1 - position
            } else {
                position
            };
            self.snapshot.folders[order[offset]].clone()
        };
        let items = (start..start.saturating_add(page_size).min(total_items))
            .map(row_at)
            .collect();
        Ok(ScanPage {
            result_id: self.snapshot.id,
            root: self.snapshot.root.clone(),
            engine: self.snapshot.engine.clone(),
            created_at_utc: self.snapshot.created_at_utc,
            items,
            total_items,
            page,
            page_size,
            total_bytes: self.totals.0,
            total_allocated_bytes: self.totals.1,
            total_files: self.totals.2,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::FolderRow;
    use chrono::Utc;
    use uuid::Uuid;

    fn sample(count: usize) -> ScanSnapshot {
        ScanSnapshot {
            id: Uuid::new_v4(),
            root: r"C:\data".to_string(),
            engine: "test".to_string(),
            created_at_utc: Utc::now(),
            links: Vec::new(),
            root_bytes: 7,
            root_allocated_bytes: 8,
            root_files: 1,
            folders: (0..count)
                .rev()
                .map(|index| FolderRow {
                    name: format!("Folder-{index:05}"),
                    full_path: format!(r"C:\data\{index}"),
                    bytes: index as u64,
                    allocated_bytes: (index / 2) as u64,
                    file_count: (index % 5) as u64,
                    link_count: 0,
                    error_count: 0,
                    is_link: false,
                    link_target: None,
                    note: (index % 3).to_string(),
                    measured: true,
                })
                .collect(),
        }
    }

    #[test]
    fn pages_cover_every_row_and_match_existing_sorting_in_both_directions() {
        let cached = CachedScan::new(sample(1503));
        for field in [
            SortField::Name,
            SortField::Bytes,
            SortField::AllocatedBytes,
            SortField::FileCount,
            SortField::Status,
        ] {
            for direction in [SortDirection::Ascending, SortDirection::Descending] {
                let mut expected = cached.snapshot.folders.clone();
                expected.sort_by(|a, b| match field {
                    SortField::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                    SortField::Bytes => a.bytes.cmp(&b.bytes),
                    SortField::AllocatedBytes => a.allocated_bytes.cmp(&b.allocated_bytes),
                    SortField::FileCount => a.file_count.cmp(&b.file_count),
                    SortField::Status => a.note.cmp(&b.note),
                });
                if matches!(direction, SortDirection::Descending) {
                    expected.reverse();
                }
                let mut actual = Vec::new();
                for page in 1..=4 {
                    let result = cached
                        .page(ScanQuery {
                            page,
                            page_size: 500,
                            sort_field: field,
                            sort_direction: direction,
                        })
                        .unwrap();
                    assert_eq!(result.total_items, 1503);
                    assert_eq!(result.total_bytes, cached.snapshot.total_bytes());
                    actual.extend(result.items);
                }
                assert_eq!(actual, expected);
            }
            assert!(Arc::ptr_eq(
                &cached.order(field).unwrap(),
                &cached.order(field).unwrap()
            ));
        }
    }

    #[test]
    fn page_bounds_are_safe_for_empty_and_out_of_range_requests() {
        for count in [0, 23] {
            let cached = CachedScan::new(sample(count));
            let page = cached
                .page(ScanQuery {
                    page: usize::MAX,
                    page_size: 0,
                    ..ScanQuery::default()
                })
                .unwrap();
            assert_eq!(page.page_size, 20);
            assert_eq!(page.page, if count == 0 { 1 } else { 2 });
            assert_eq!(page.items.len(), if count == 0 { 0 } else { 3 });
        }
    }
}
