use crate::error::Result;
use crate::formats::Service;
use std::collections::HashMap;
use std::hash::Hash;

/// Get a starting executon order
/// Result is a vector of vectors(batches) of Service.
/// Each batch has no dependence between each other so they can be started in parallel.
/// TODO: this might deadlock if `services` is not a DAG.
pub fn topological_sort(services: Vec<Service>) -> Result<Vec<Vec<Service>>> {
    // Keep a service_name : Service map.
    let mut name_serv = services
        .iter()
        .cloned()
        .map(|srv| (srv.name.clone(), srv))
        .collect::<HashMap<String, Service>>();
    // Create an adjacency list: Service -> [ services ]
    let mut adj_list = services
        .iter()
        .cloned()
        .fold(HashMap::new(), |mut acc, srv| {
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

    // Count the indegree of each node
    let mut indegree: HashMap<String, u64> = services
        .into_iter()
        .map(|srv| (srv.name, srv.start_after.len() as u64))
        .collect();

    let mut ret: Vec<Vec<Service>> = Vec::new();
    println!("Indegree map: {:#?}, adj_list: {:#?}", indegree, adj_list);
    println!("Calculating the execution order...");

    while !indegree.is_empty() {
        println!("Indegree not empty! Indegree: {:?}", indegree);
        let (no_deps, new_indegree) = indegree
            .into_iter()
            .partition(|(name, indegree)| *indegree == 0);
        indegree = new_indegree;

        let as_v: Vec<Service> = no_deps
            .keys()
            .map(|v| name_serv.get(v).unwrap().to_owned())
            .collect();
        ret.push(as_v);
        // Update adj_list, by removing no_deps nodes. Lower indegree for each dependant service.
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

    #[test]
    pub fn test_top_sort() -> Result<()> {
        let a = Service::from_name("a");
        let b = Service::start_after("b", vec!["a"]);

        let res = topological_sort(vec![a.clone(), b.clone()])?;
        let expected = vec![vec![a.clone()], vec![b.clone()]];
        assert_eq!(res, expected, "a -> b");

        let c = Service::start_after("c", vec!["a"]);
        let res = topological_sort(vec![a.clone(), b.clone(), c.clone()])?;
        let expected = vec![vec![a.clone()], vec![b.clone(), c.clone()]];
        assert_eq!(res, expected, "(a-> (b, c)");

        let d = Service::from_name("d");
        let e = Service::from_name("e");

        let res = topological_sort(vec![a.clone(), d.clone(), e.clone()])?;
        let expected = vec![vec![a.clone(), e.clone(), d.clone()]];
        assert_eq!(res, expected, "(a,e,d)");
        Ok(())
    }
}
