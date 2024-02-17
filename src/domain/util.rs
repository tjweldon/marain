use std::{collections::hash_map::DefaultHasher, hash::{Hash, Hasher}};

pub fn hash(to_be_hashed: String) -> u64 {
    let mut hasher = DefaultHasher::new();
    to_be_hashed.hash(&mut hasher);
    hasher.finish()
}