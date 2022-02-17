// Copyright 2021 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use bee_storage::{
    access::{AsIterator, Fetch, MultiFetch},
    backend,
    system::{StorageHealth, System, SYSTEM_HEALTH_KEY, SYSTEM_VERSION_KEY},
};

use std::collections::HashMap;

pub trait StorageBackend:
    backend::StorageBackend + Fetch<u8, System> + for<'a> MultiFetch<'a, u8, System> + for<'a> AsIterator<'a, u8, System>
{
}

impl<S> StorageBackend for S where
    S: backend::StorageBackend
        + Fetch<u8, System>
        + for<'a> MultiFetch<'a, u8, System>
        + for<'a> AsIterator<'a, u8, System>
{
}

/// Generic access tests for the system table.
pub fn system_access<S: StorageBackend>(storage: &S) {
    let version = Fetch::<u8, System>::fetch(storage, &SYSTEM_VERSION_KEY)
        .unwrap()
        .unwrap();
    assert_eq!(version, System::Version(storage.version().unwrap().unwrap()));

    let health = Fetch::<u8, System>::fetch(storage, &SYSTEM_HEALTH_KEY)
        .unwrap()
        .unwrap();
    assert_eq!(health, System::Health(storage.health().unwrap().unwrap()));
    assert_eq!(health, System::Health(StorageHealth::Idle));

    assert_eq!(Fetch::<u8, System>::fetch(storage, &42).unwrap(), None);

    let systems = MultiFetch::<u8, System>::multi_fetch(storage, &[SYSTEM_VERSION_KEY, SYSTEM_HEALTH_KEY, 42])
        .unwrap()
        .collect::<Vec<_>>();
    assert_eq!(systems[0].as_ref().unwrap().unwrap(), version);
    assert_eq!(systems[1].as_ref().unwrap().unwrap(), health);
    assert_eq!(systems[2].as_ref().unwrap(), &None);

    let mut systems = HashMap::new();
    systems.insert(SYSTEM_VERSION_KEY, version);
    systems.insert(SYSTEM_HEALTH_KEY, health);

    let iter = AsIterator::<u8, System>::iter(storage).unwrap();
    let mut count = 0;

    for result in iter {
        let (key, value) = result.unwrap();
        assert_eq!(systems.get(&key), Some(&value));
        count += 1;
    }

    assert_eq!(count, systems.len());

    storage.set_health(StorageHealth::Corrupted).unwrap();
    assert_eq!(
        System::Health(storage.health().unwrap().unwrap()),
        System::Health(StorageHealth::Corrupted)
    );
    assert_eq!(
        Fetch::<u8, System>::fetch(storage, &SYSTEM_HEALTH_KEY)
            .unwrap()
            .unwrap(),
        System::Health(StorageHealth::Corrupted)
    );
}