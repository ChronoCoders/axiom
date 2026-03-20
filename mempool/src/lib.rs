use axiom_crypto::compute_transaction_hash_for_height;
use axiom_primitives::{Transaction, TransactionHash};
use std::collections::{HashMap, VecDeque};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MempoolError {
    #[error("Mempool is full")]
    Full,
    #[error("Transaction already exists")]
    Duplicate,
}

pub struct Mempool {
    transactions: HashMap<TransactionHash, Transaction>,
    queue: VecDeque<TransactionHash>,
    capacity: usize,
}

impl Mempool {
    pub fn new(capacity: usize) -> Self {
        Self {
            transactions: HashMap::new(),
            queue: VecDeque::new(),
            capacity,
        }
    }

    /// Adds a transaction to the mempool.
    ///
    /// # Validation
    /// This method does NOT validate the transaction signature.
    /// It assumes the caller (API or Network layer) has already performed cryptographic verification.
    ///
    /// Returns error if full or duplicate.
    pub fn add(&mut self, tx: Transaction) -> Result<(), MempoolError> {
        let hash = compute_transaction_hash_for_height(0, &tx);

        if self.transactions.contains_key(&hash) {
            return Err(MempoolError::Duplicate);
        }

        if self.transactions.len() >= self.capacity {
            return Err(MempoolError::Full);
        }

        self.transactions.insert(hash, tx);
        self.queue.push_back(hash);
        Ok(())
    }

    pub fn add_for_height(&mut self, height: u64, tx: Transaction) -> Result<(), MempoolError> {
        let hash = compute_transaction_hash_for_height(height, &tx);

        if self.transactions.contains_key(&hash) {
            return Err(MempoolError::Duplicate);
        }

        if self.transactions.len() >= self.capacity {
            return Err(MempoolError::Full);
        }

        self.transactions.insert(hash, tx);
        self.queue.push_back(hash);
        Ok(())
    }

    /// Returns a batch of transactions (up to limit), preserving FIFO order.
    pub fn get_batch(&self, limit: usize) -> Vec<Transaction> {
        let mut batch = Vec::new();
        for hash in self.queue.iter().take(limit) {
            if let Some(tx) = self.transactions.get(hash) {
                batch.push(tx.clone());
            }
        }
        batch
    }

    /// Removes a batch of transactions from the mempool (e.g., after block commit).
    pub fn remove_batch(&mut self, hashes: &[TransactionHash]) {
        for hash in hashes {
            if self.transactions.remove(hash).is_some() {
                // Remove from queue - this is O(N) but acceptable for V1/simple mempool
                // In a production system, we'd use a better structure or lazy removal
                if let Some(pos) = self.queue.iter().position(|h| h == hash) {
                    self.queue.remove(pos);
                }
            }
        }
    }

    pub fn size(&self) -> usize {
        self.transactions.len()
    }

    pub fn contains(&self, hash: &TransactionHash) -> bool {
        self.transactions.contains_key(hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_crypto::{compute_transaction_hash_for_height, sign_transaction, test_keypair};
    use axiom_primitives::{AccountId, Signature, Transaction, TransactionHash, TransactionType};

    fn create_dummy_tx(nonce: u64) -> Transaction {
        let (sk, pk) = test_keypair("sender");
        let (_, dest_pk) = test_keypair("dest");
        let sender = AccountId(pk.0);
        let recipient = AccountId(dest_pk.0);

        let mut tx = Transaction {
            sender,
            recipient,
            amount: 10,
            nonce,
            signature: Signature([0u8; 64]),
            tx_type: TransactionType::Transfer,
            evidence: None,
        };

        let sig = sign_transaction(&sk, &tx);
        tx.signature = sig;
        tx
    }

    #[test]
    fn test_add_transaction() {
        let mut pool = Mempool::new(10);
        let tx = create_dummy_tx(0);
        assert!(pool.add(tx).is_ok());
        assert_eq!(pool.size(), 1);
    }

    #[test]
    fn test_duplicate_transaction() {
        let mut pool = Mempool::new(10);
        let tx = create_dummy_tx(0);
        pool.add(tx.clone()).unwrap();
        assert!(matches!(pool.add(tx), Err(MempoolError::Duplicate)));
    }

    #[test]
    fn test_capacity_limit() {
        let mut pool = Mempool::new(2);
        pool.add(create_dummy_tx(1)).unwrap();
        pool.add(create_dummy_tx(2)).unwrap();
        assert!(matches!(
            pool.add(create_dummy_tx(3)),
            Err(MempoolError::Full)
        ));
    }

    #[test]
    fn test_fifo_ordering() {
        let mut pool = Mempool::new(10);
        let tx1 = create_dummy_tx(1);
        let tx2 = create_dummy_tx(2);
        pool.add(tx1.clone()).unwrap();
        pool.add(tx2.clone()).unwrap();

        let batch = pool.get_batch(2);
        assert_eq!(
            compute_transaction_hash_for_height(0, &batch[0]),
            compute_transaction_hash_for_height(0, &tx1)
        );
        assert_eq!(
            compute_transaction_hash_for_height(0, &batch[1]),
            compute_transaction_hash_for_height(0, &tx2)
        );
    }

    #[test]
    fn test_remove_batch() {
        let mut pool = Mempool::new(10);
        let tx1 = create_dummy_tx(1);
        let tx2 = create_dummy_tx(2);
        let hash1 = compute_transaction_hash_for_height(0, &tx1);

        pool.add(tx1).unwrap();
        pool.add(tx2).unwrap();

        pool.remove_batch(std::slice::from_ref(&hash1));
        assert!(!pool.contains(&hash1));
        assert_eq!(pool.size(), 1);
    }

    #[test]
    fn test_get_batch_exceeding_size() {
        let mut pool = Mempool::new(10);
        pool.add(create_dummy_tx(1)).unwrap();
        pool.add(create_dummy_tx(2)).unwrap();

        // Request 5, only have 2
        let batch = pool.get_batch(5);
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn test_get_batch_empty() {
        let pool = Mempool::new(10);
        let batch = pool.get_batch(5);
        assert!(batch.is_empty());
    }

    #[test]
    fn test_remove_batch_unknown() {
        let mut pool = Mempool::new(10);
        let tx = create_dummy_tx(1);
        pool.add(tx).unwrap();

        let unknown_hash = TransactionHash([0u8; 32]);
        pool.remove_batch(&[unknown_hash]);

        assert_eq!(pool.size(), 1); // Should not remove anything
    }

    #[test]
    fn test_contains_non_existent() {
        let pool = Mempool::new(10);
        let unknown_hash = TransactionHash([0u8; 32]);
        assert!(!pool.contains(&unknown_hash));
    }
}
