

macro_rules! fixed_size_list {
    ( $struct_name:ident: $contained:ty ; $capacity:expr $(; $meta:meta),* ) => {
        $(#[$meta])*
        pub(crate) struct $struct_name {
            _size: usize,
            _field: [Option<$contained>; $capacity],
        }

        impl $struct_name {
            #[allow(dead_code)]
            pub(crate) fn empty() -> $struct_name {
                $struct_name {
                    _size: 0,
                    _field: [None; $capacity],
                }
            }

            #[allow(dead_code)]
            pub(crate) fn capacity() -> usize {
                $capacity
            }

            #[allow(dead_code)]
            pub(crate) fn push(&mut self, item: $contained) -> bool {
                if $struct_name::capacity() > self._size {
                    self._field[self._size] = Some(item);
                    self._size += 1;
                    true
                } else {
                    false
                }
            }

            #[allow(dead_code)]
            pub(crate) fn contains(&self, item: &$contained) -> bool {
                for i in 0..self._size {
                    if &self._field[i].unwrap() == item {
                        return true;
                    }
                }
                false
            }

            #[allow(dead_code)]
            pub(crate) fn pop(&mut self) -> Option<$contained> {
                let result = self._field[self._size];
                self._size = if self._size > 0 { self._size - 1 } else { 0 };
                result
            }

            #[allow(dead_code)]
            pub(crate) fn copy_to(&self, to: &mut [$contained]) {
                for i in 0..self._size {
                    to[i] = self._field[i].unwrap();
                }
            }
        }

        impl From<Vec<$contained>> for $struct_name {
            fn from(rs: Vec<$contained>) -> Self {
                let mut result = $struct_name::empty();
                for (i, r) in rs.iter().enumerate() {
                    result._field[i] = Some(r.clone());
                }
                result._size = rs.len();
                result
            }
        }

        impl Into<Vec<$contained>> for $struct_name {
            fn into(self) -> Vec<$contained> {
                let mut result = Vec::with_capacity(self._size);
                for item in self._field.iter() {
                    match item {
                        &Some(i) => result.push(i),
                        &None => return result,
                    }
                }
                result
            }
        }
    }
    
}

