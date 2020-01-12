use crate::error::Result;
use crate::formats::Service;
use std::collections::HashMap;
use std::hash::Hash;

/**
   B -> A, C
   A-> B
   C -> B
   {
        B: 2
        A: 0
   [B, C

   A -> B


*/

/// Get a starting executon order
/// Result is a vector of vectors(batches) of Service.
/// Each batch has no dependence between each other so they can be started in parallel.
pub fn topological_sort(serv: Vec<Service>) -> Result<Vec<Vec<Service>>> {
    // Keep a service_name : Service map.
    let mut name_serv = serv
        .iter()
        .cloned()
        .map(|srv| (srv.name.clone(), srv))
        .collect::<HashMap<String, Service>>();
    // Create an adjacency list: Service -> [ services ]
    let mut adj_list = serv.iter().cloned().fold(HashMap::new(), |mut acc, srv| {
        let service_name = srv.name.clone();
        if !acc.contains_key(&service_name) {
            acc.insert(service_name.clone(), Vec::new());
        }
        srv.start_after.into_iter().for_each(|dep| {
            let mut old_adjs: Vec<String> = acc.remove(&dep).unwrap_or(Vec::new());
            old_adjs.push(service_name.clone());
            acc.insert(dep, old_adjs);
        });
        acc
    });
    //.collect::<HashMap<String, Vec<String>>>();

    // Count the indegree of each node
    let mut indegree: HashMap<String, u64> = serv
        .into_iter()
        .map(|srv| (srv.name, srv.start_after.len() as u64))
        .collect();

    let mut ret: Vec<Vec<Service>> = Vec::new();
    println!("Indegree map: {:#?}, adj_list: {:#?}", indegree, adj_list);
    println!("Calculating the execution order...");

    while !indegree.is_empty() {
        println!("Indegree not empty! Indegree: {:?}", indegree);
        let (no_deps, new_indegree): (HashMap<String, u64>, HashMap<String, u64>) = indegree
            .into_iter()
            .partition(|(name, indegree)| *indegree == 0);
        indegree = new_indegree;

        let as_v: Vec<Service> = no_deps
            .keys()
            .map(|v| name_serv.get(v).unwrap().to_owned())
            .collect();
        ret.push(as_v);
        // Update adj_list, by removing no_deps nodes.
        println!("No_deps: {:?}", no_deps);
        no_deps.into_iter().for_each(|(name, _zero_deg)| {
            let dependants = adj_list.remove(&name).unwrap();
            dependants.into_iter().for_each(|s| {
                let old_degree = indegree.remove(&s).unwrap();
                indegree.insert(s, old_degree - 1);
            })
        });
    }

    Ok(ret)
}

#[cfg(test)]
mod test {
    use crate::error::Result;
    use crate::formats::{RestartStrategy, Service};
    use crate::runtime::topological_sort;
    use std::time::Duration;

    impl Service {
        pub fn start_after(name: &str, start_after: Vec<&str>) -> Self {
            Service {
                name: name.to_owned(),
                start_after: start_after.into_iter().map(|v| v.into()).collect(),
                path: "".into(),
                restart: RestartStrategy::Always,
                start_delay: Duration::from_secs(0),
            }
        }
        fn from_name(name: &str) -> Self {
            Service {
                name: name.to_owned(),
                start_after: Vec::new(),
                path: "".into(),
                restart: RestartStrategy::Always,
                start_delay: Duration::from_secs(0),
            }
        }
    }
    #[test]
    pub fn test_top_sort() -> Result<()> {
        let a = Service::from_name("a");
        let b = Service::start_after("b", vec!["a"]);
        let simple = vec![a.clone(), b.clone()];
        let res = topological_sort(simple)?;
        let expected = vec![vec![a], vec![b]];
        assert_eq!(res, expected);
        Ok(())
    }
}
