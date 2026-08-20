//! Randomized node iteration matching C# `RandomServers`.

pub(crate) struct RandomNodes {
    urls: Vec<String>,
    order: Vec<usize>,
    next: usize,
}

impl RandomNodes {
    pub(crate) fn new(nodes: &[String]) -> Self {
        let urls = nodes.to_vec();
        let len = urls.len();
        let start = if len == 0 { 0 } else { random_index(len) };
        let mut order = Vec::with_capacity(len);
        if len > 0 {
            for offset in 0..len {
                order.push((start + offset) % len);
            }
        }
        Self {
            urls,
            order,
            next: 0,
        }
    }
}

impl Iterator for RandomNodes {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        let index = self.order.get(self.next).copied()?;
        self.next += 1;
        self.urls.get(index).cloned()
    }
}

fn random_index(len: usize) -> usize {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    (nanos % u128::from(u64::try_from(len).unwrap_or(1)))
        .try_into()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::RandomNodes;

    #[test]
    fn iterates_each_node_once() {
        let nodes = vec!["http://a".into(), "http://b".into(), "http://c".into()];
        let mut seen = RandomNodes::new(&nodes).collect::<Vec<_>>();
        seen.sort();
        let mut expected = nodes;
        expected.sort();
        assert_eq!(seen, expected);
    }

    #[test]
    fn empty_nodes_yield_nothing() {
        assert_eq!(RandomNodes::new(&[]).count(), 0);
    }
}
