//! Godot's `Array.sort_custom`, which isn't stable: `SortArray` (introsort: quicksort
//! with a median-of-three pivot, then one insertion-sort pass; a heap sort when the
//! recursion gets too deep). Elements that compare equal come out in Godot's order, not
//! in their input order, so where the GDScript sorts a list with ties (equally distant
//! lots) the Rust port sorts it through here.
//!
//! `less(a, b)` is the GDScript comparator (a strict "a comes before b").

const INTROSORT_THRESHOLD: usize = 16;

/// `Array.sort_custom(less)`.
pub fn sort_custom<T: Clone>(array: &mut [T], mut less: impl FnMut(&T, &T) -> bool) {
    let len = array.len();
    if len == 0 {
        return;
    }
    introsort(array, 0, len, bitlog(len) * 2, &mut less);
    if len > INTROSORT_THRESHOLD {
        insertion_sort(array, 0, INTROSORT_THRESHOLD, &mut less);
        for i in INTROSORT_THRESHOLD..len {
            unguarded_linear_insert(array, i, array[i].clone(), &mut less);
        }
    } else {
        insertion_sort(array, 0, len, &mut less);
    }
}

fn bitlog(mut n: usize) -> usize {
    let mut k = 0;
    while n != 1 {
        n >>= 1;
        k += 1;
    }
    k
}

fn median_of_3<T: Clone>(a: &T, b: &T, c: &T, less: &mut impl FnMut(&T, &T) -> bool) -> T {
    if less(a, b) {
        if less(b, c) {
            b.clone()
        } else if less(a, c) {
            c.clone()
        } else {
            a.clone()
        }
    } else if less(a, c) {
        a.clone()
    } else if less(b, c) {
        c.clone()
    } else {
        b.clone()
    }
}

fn partitioner<T: Clone>(
    array: &mut [T],
    mut first: usize,
    mut last: usize,
    pivot: &T,
    less: &mut impl FnMut(&T, &T) -> bool,
) -> usize {
    loop {
        while less(&array[first], pivot) {
            first += 1;
        }
        last -= 1;
        while less(pivot, &array[last]) {
            last -= 1;
        }
        if first >= last {
            return first;
        }
        array.swap(first, last);
        first += 1;
    }
}

fn introsort<T: Clone>(
    array: &mut [T],
    first: usize,
    mut last: usize,
    mut max_depth: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    while last - first > INTROSORT_THRESHOLD {
        if max_depth == 0 {
            // Godot's partial_sort (a heap sort). Never reached by the sizes the sim
            // sorts; a stable sort stands in for it.
            let mut part = array[first..last].to_vec();
            part.sort_by(|a, b| {
                if less(a, b) {
                    std::cmp::Ordering::Less
                } else if less(b, a) {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            });
            array[first..last].clone_from_slice(&part);
            return;
        }
        max_depth -= 1;
        let pivot = median_of_3(
            &array[first],
            &array[first + (last - first) / 2],
            &array[last - 1],
            less,
        );
        let cut = partitioner(array, first, last, &pivot, less);
        introsort(array, cut, last, max_depth, less);
        last = cut;
    }
}

fn insertion_sort<T: Clone>(
    array: &mut [T],
    first: usize,
    last: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    if first == last {
        return;
    }
    for i in first + 1..last {
        let value = array[i].clone();
        if less(&value, &array[first]) {
            for j in (first + 1..=i).rev() {
                array[j] = array[j - 1].clone();
            }
            array[first] = value;
        } else {
            unguarded_linear_insert(array, i, value, less);
        }
    }
}

fn unguarded_linear_insert<T: Clone>(
    array: &mut [T],
    mut last: usize,
    value: T,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    let mut next = last - 1;
    while less(&value, &array[next]) {
        array[last] = array[next].clone();
        last = next;
        next = next.wrapping_sub(1);
    }
    array[last] = value;
}

#[cfg(test)]
mod tests {
    use super::sort_custom;

    #[test]
    fn sorts() {
        let mut v: Vec<i32> = (0..100).map(|i| (i * 37) % 101).collect();
        sort_custom(&mut v, |a, b| a < b);
        assert!(v.windows(2).all(|w| w[0] <= w[1]));
        let mut small = vec![3, 1, 2];
        sort_custom(&mut small, |a, b| a < b);
        assert_eq!(small, vec![1, 2, 3]);
    }
}
