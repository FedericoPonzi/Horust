mod proto;
use proto::tutorial::*;

pub fn create_large_shirt(color: String) -> Person {
    let mut person = Person::default();
    person
}

pub fn add() {
    println!("{:?}", create_large_shirt("blue".to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        add();
    }
}
