use chrono::{DateTime, Duration, Local};

/// Трейт элемента коллекции
pub trait FarLongIteratorItem: Clone {
    /// расстояние в пространстве до другого такого-же элемента
    fn distance(&self, other: &Self) -> u64;

    /// Возвращает штамп времени последнего выбора канала
    fn last_selected(&self) -> DateTime<Local>;

    /// Элемент валиден?
    fn is_valid(&self) -> bool;
}

/// бесконечный итератор на слайсе, который возвращает элемент максимально далекий от превыдущего как во времени так и в пространстве
pub struct FarLongIterator<T: FarLongIteratorItem> {
    elemnts: Vec<T>,
    current_selected: Option<usize>,
    time_tolerance: Duration,
}

impl<T: FarLongIteratorItem> FarLongIterator<T> {
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.elemnts.get_mut(index)
    }

    pub fn reset(&mut self) {
        self.current_selected = None;
    }

    pub fn len(&self) -> usize {
        self.elemnts.len()
    }
}

pub trait IntoFarLongIterator<T: FarLongIteratorItem> {
    fn into_far_long_iterator(self, time_tolerance: Duration) -> FarLongIterator<T>;
}

impl<T: FarLongIteratorItem> IntoFarLongIterator<T> for Vec<T> {
    fn into_far_long_iterator(self, time_tolerance: Duration) -> FarLongIterator<T> {
        FarLongIterator {
            elemnts: self,
            current_selected: None,
            time_tolerance,
        }
    }
}

impl<T: FarLongIteratorItem> Iterator for FarLongIterator<T> {
    type Item = usize;

    /// Возвращает элемент максимально далекий от превыдущего как во времени так и в пространстве, бесконечно повторяясь, пока есть хоть один валидный элемент в коллекции
    fn next(&mut self) -> Option<Self::Item> {
        struct ItemDistance {
            index: usize,
            distance: u64,
            last_selected: DateTime<Local>,
        }

        if let Some(c) = self.current_selected {
            let mut distances = self
                .elemnts
                .iter()
                .enumerate()
                .filter_map(|(i, item)| {
                    if item.is_valid() {
                        Some(ItemDistance {
                            index: i,
                            distance: item.distance(&self.elemnts[c]),
                            last_selected: item.last_selected(),
                        })
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            // сортировка по времени последнего выбора
            distances.sort_by(move |a, b| b.last_selected.cmp(&a.last_selected));
            if let Some(oldest) = distances.last() {
                let older_time = oldest.last_selected + self.time_tolerance;
                distances.retain(|item| item.last_selected < older_time);
                distances.sort_by(|a, b| a.distance.cmp(&b.distance));
                distances.last().map(|item| {
                    self.current_selected.replace(item.index);
                    item.index
                })
            } else {
                None
            }
        } else {
            if let Some(i) = self
                .elemnts
                .iter_mut()
                .enumerate()
                .filter_map(|(i, item)| if item.is_valid() { Some(i) } else { None })
                .next()
            {
                self.current_selected.replace(i);
                Some(i)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Clone)]
    struct Item<const TOTAL: u32> {
        id: u32,
        last_selected: DateTime<Local>,
        select_count: usize,
    }

    impl<const TOTAL: u32> Item<TOTAL> {
        fn new(id: u32) -> Self {
            Self {
                id,
                last_selected: Local::now(),
                select_count: 0,
            }
        }

        fn priv_distance(&self, other: &Self) -> u32 {
            let forward_distance = if self.id > other.id {
                self.id - other.id
            } else {
                other.id - self.id
            };
            let wraped_distance = if self.id > other.id {
                other.id + TOTAL - self.id
            } else {
                self.id + TOTAL - other.id
            };

            std::cmp::min(forward_distance, wraped_distance)
        }

        fn select(&mut self) {
            self.last_selected = Local::now();
            self.select_count += 1;
        }
    }

    impl<const TOTAL: u32> FarLongIteratorItem for Item<TOTAL> {
        fn distance(&self, other: &Self) -> u64 {
            self.priv_distance(other) as u64
        }

        fn last_selected(&self) -> DateTime<Local> {
            self.last_selected
        }

        fn is_valid(&self) -> bool {
            self.select_count < TOTAL as usize
        }
    }

    #[test]
    fn test_iterator5() {
        const SIZE: u32 = 5;

        type SItem = Item<SIZE>;

        let correct_order = vec![0, 2, 4, 1, 3];
        let elemnts = (0..SIZE).map(SItem::new).collect::<Vec<_>>();
        let duraton = Duration::microseconds(100);

        let mut iterator = elemnts.into_far_long_iterator(duraton);

        let mut i = 0;
        while let Some(id) = iterator.next() {
            assert_eq!(id, correct_order[i % SIZE as usize]);
            std::thread::sleep((duraton * 2).to_std().unwrap());
            iterator.get_mut(id).map(SItem::select);
            i += 1;
        }
    }

    #[test]
    fn test_iterator16() {
        const SIZE: u32 = 16;

        type SItem = Item<SIZE>;

        let correct_order = vec![0, 8, 1, 9, 2, 10, 3, 11, 4, 12, 5, 13, 6, 14, 7, 15];
        let elemnts = (0..SIZE).map(SItem::new).collect::<Vec<_>>();
        let duraton = Duration::microseconds(100);

        let mut iterator = elemnts.into_far_long_iterator(Duration::microseconds(100));

        let mut i = 0;
        while let Some(id) = iterator.next() {
            assert_eq!(id, correct_order[i % SIZE as usize]);
            std::thread::sleep((duraton * 2).to_std().unwrap());
            iterator.get_mut(id).map(SItem::select);
            i += 1;
        }
    }
}
