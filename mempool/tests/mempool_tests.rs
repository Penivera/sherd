use std::sync::Arc;
use mempool::{Mempool, MempoolError, Task, TaskState};
use wire::{NodeId, TaskId};

#[test]
fn test_mempool_submit_and_get() {
    let pool = Mempool::new();
    let author = NodeId::new([1u8; 32]);
    let task = Task::new_available(author, 10, 1000, b"job_1".to_vec());
    let id = task.id;

    assert!(!pool.contains(&id));
    assert_eq!(pool.len(), 0);

    let submitted_id = pool.submit(task.clone()).expect("Submit task");
    assert_eq!(id, submitted_id);
    assert!(pool.contains(&id));
    assert_eq!(pool.len(), 1);

    let retrieved = pool.get(&id).expect("Get task");
    assert_eq!(retrieved.id, id);
    assert_eq!(retrieved.author, author);
    assert_eq!(retrieved.state, TaskState::Available);
}

#[test]
fn test_mempool_rejects_duplicates() {
    let pool = Mempool::new();
    let author = NodeId::new([2u8; 32]);
    let task = Task::new_available(author, 5, 2000, b"job_dup".to_vec());

    pool.submit(task.clone()).expect("First submit succeeds");
    let result = pool.submit(task);
    assert!(matches!(result, Err(MempoolError::DuplicateTask(_))));
    assert_eq!(pool.len(), 1);
}

#[test]
fn test_deterministic_task_ordering() {
    let pool = Mempool::new();
    let author = NodeId::new([3u8; 32]);

    // Create tasks with different priorities and timestamps
    let task_low = Task::new_available(author, 1, 100, b"low_pri".to_vec());
    let task_high_old = Task::new_available(author, 10, 100, b"high_old".to_vec());
    let task_high_new = Task::new_available(author, 10, 200, b"high_new".to_vec());
    let task_med = Task::new_available(author, 5, 50, b"med_pri".to_vec());

    pool.submit(task_low.clone()).unwrap();
    pool.submit(task_high_new.clone()).unwrap();
    pool.submit(task_high_old.clone()).unwrap();
    pool.submit(task_med.clone()).unwrap();

    let ordered = pool.list_available();
    assert_eq!(ordered.len(), 4);

    // Expected order:
    // 1. high_old (priority 10, time 100)
    // 2. high_new (priority 10, time 200)
    // 3. med (priority 5, time 50)
    // 4. low (priority 1, time 100)
    assert_eq!(ordered[0].id, task_high_old.id);
    assert_eq!(ordered[1].id, task_high_new.id);
    assert_eq!(ordered[2].id, task_med.id);
    assert_eq!(ordered[3].id, task_low.id);
}

#[test]
fn test_task_lifecycle_transitions() {
    let pool = Mempool::new();
    let author = NodeId::new([4u8; 32]);
    let worker = NodeId::new([5u8; 32]);
    let task = Task::new_available(author, 10, 500, b"lifecycle".to_vec());
    let id = task.id;

    pool.submit(task).unwrap();

    // Cannot complete without claiming first
    assert!(matches!(
        pool.complete(&id, b"result".to_vec(), 600),
        Err(MempoolError::InvalidStateTransition { .. })
    ));

    // Claim succeeds
    pool.claim(&id, worker, 550).expect("Claim task");
    let claimed = pool.get(&id).unwrap();
    assert_eq!(
        claimed.state,
        TaskState::Claimed {
            by: worker,
            claimed_at: 550
        }
    );

    // Second claim fails
    let worker_2 = NodeId::new([6u8; 32]);
    assert!(matches!(
        pool.claim(&id, worker_2, 560),
        Err(MempoolError::AlreadyClaimed { .. })
    ));

    // Complete succeeds
    pool.complete(&id, b"computed_output".to_vec(), 570)
        .expect("Complete task");
    let completed = pool.get(&id).unwrap();
    assert_eq!(
        completed.state,
        TaskState::Completed {
            completed_at: 570,
            result: b"computed_output".to_vec()
        }
    );

    // Cannot claim after completion
    assert!(matches!(
        pool.claim(&id, worker, 580),
        Err(MempoolError::InvalidStateTransition { .. })
    ));
}

#[test]
fn test_concurrent_claiming_atomicity() {
    let pool = Arc::new(Mempool::new());
    let author = NodeId::new([0xaa; 32]);
    let task = Task::new_available(author, 10, 1000, b"concurrency_target".to_vec());
    let id = task.id;

    pool.submit(task).unwrap();

    let mut handles = Vec::new();
    let claimants_count = 10;

    for i in 0..claimants_count {
        let pool_clone = Arc::clone(&pool);
        let claimant = NodeId::new([i as u8; 32]);
        handles.push(std::thread::spawn(move || {
            pool_clone.claim(&id, claimant, 1010 + i as u64)
        }));
    }

    let mut success_count = 0;
    let mut already_claimed_count = 0;

    for handle in handles {
        match handle.join().unwrap() {
            Ok(()) => success_count += 1,
            Err(MempoolError::AlreadyClaimed { .. }) => already_claimed_count += 1,
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    assert_eq!(success_count, 1, "Exactly one claim must succeed");
    assert_eq!(
        already_claimed_count,
        claimants_count - 1,
        "All other claims must fail with AlreadyClaimed"
    );
}

#[test]
fn test_pending_to_available_payload_resolution() {
    let pool = Mempool::new();
    let author = NodeId::new([0x33; 32]);
    let raw_payload = b"actual_work_items".to_vec();
    let expected_id = TaskId::compute(&author, 5, 200, &raw_payload);

    let pending_task = Task::new_pending(expected_id, author, 5, 200);
    pool.submit(pending_task).unwrap();

    assert_eq!(pool.list_available().len(), 0);

    // Bad payload fails hash verification
    let bad_payload = b"tampered_work_items".to_vec();
    assert!(matches!(
        pool.set_payload(&expected_id, bad_payload),
        Err(MempoolError::InvalidTaskHash { .. })
    ));

    // Correct payload succeeds
    pool.set_payload(&expected_id, raw_payload.clone()).unwrap();
    let ready = pool.get(&expected_id).unwrap();
    assert_eq!(ready.state, TaskState::Available);
    assert_eq!(ready.payload, raw_payload);
    assert_eq!(pool.list_available().len(), 1);
}
