#![allow(dead_code)]

use tempfile::TempDir;
use sled::Db;

fn create_test_db(temp_dir: &TempDir) -> sled::Db {
    sled::open(temp_dir.path()).unwrap()
}

#[test]
fn test_open_db() {
    let temp_dir = TempDir::new().unwrap();
    let _db = create_test_db(&temp_dir);
}

#[test]
fn test_add_pubkey() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir);
    
    let tree = db.open_tree("whitelist").unwrap();
    let pubkey = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    
    tree.insert(pubkey.as_bytes(), vec![]).unwrap();
    
    let contains = tree.contains_key(pubkey.as_bytes()).unwrap();
    assert!(contains);
}

#[test]
fn test_add_duplicate() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir);
    
    let tree = db.open_tree("whitelist").unwrap();
    let pubkey = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    
    tree.insert(pubkey.as_bytes(), vec![]).unwrap();
    let was_present = tree.contains_key(pubkey.as_bytes()).unwrap();
    
    assert!(was_present);
}

#[test]
fn test_remove_pubkey() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir);
    
    let tree = db.open_tree("whitelist").unwrap();
    let pubkey = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    
    tree.insert(pubkey.as_bytes(), vec![]).unwrap();
    let removed = tree.remove(pubkey.as_bytes()).unwrap();
    
    assert!(removed.is_some());
}

#[test]
fn test_remove_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir);
    
    let tree = db.open_tree("whitelist").unwrap();
    let pubkey = "0000000000000000000000000000000000000000000000000000000000000";
    
    let removed = tree.remove(pubkey.as_bytes()).unwrap();
    
    assert!(removed.is_none());
}

#[test]
fn test_list_pubkeys() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir);
    
    {
        let tree = db.open_tree("whitelist").unwrap();
        tree.insert(b"aaa", vec![]).unwrap();
        tree.insert(b"bbb", vec![]).unwrap();
        tree.insert(b"ccc", vec![]).unwrap();
    }
    
    let tree = db.open_tree("whitelist").unwrap();
    let mut pubkeys: Vec<String> = Vec::new();
    for key_result in tree.iter() {
        let (key, _) = key_result.unwrap();
        pubkeys.push(String::from_utf8(key.to_vec()).unwrap());
    }
    pubkeys.sort();
    
    assert_eq!(pubkeys.len(), 3);
    assert_eq!(pubkeys[0], "aaa");
    assert_eq!(pubkeys[1], "bbb");
    assert_eq!(pubkeys[2], "ccc");
}

#[test]
fn test_contains() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir);
    
    {
        let tree = db.open_tree("whitelist").unwrap();
        tree.insert(b"abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789", vec![]).unwrap();
    }
    
    let tree = db.open_tree("whitelist").unwrap();
    let contains = tree.contains_key(b"abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789").unwrap();
    
    assert!(contains);
}

#[test]
fn test_not_contains() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir);
    
    let tree = db.open_tree("whitelist").unwrap();
    let contains = tree.contains_key(b"0000000000000000000000000000000000000000000000000000000000000").unwrap();
    
    assert!(!contains);
}