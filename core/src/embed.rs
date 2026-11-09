//! Embedding interface. Models are versioned assets behind this trait (T8);
//! the core never hard-codes a model. The production runtime is
//! [`crate::bert::BertEmbedder`].

use crate::error::{CoreError, Result};

pub trait Embedder: Send + Sync {
    /// Versioned id, e.g. `"bge-small-en-v1.5@1"`. Stored with every vector so a
    /// model change triggers a re-embed rather than mixing vector spaces.
    fn model_id(&self) -> &str;
    fn dims(&self) -> usize;
    /// Embed passages (chunks).
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    /// Embed a search query. Models with a retrieval instruction override this.
    fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        self.embed(&[query])?
            .into_iter()
            .next()
            .ok_or_else(|| CoreError::Embedding("empty query embedding".into()))
    }
}

/// Cosine similarity. Returns 0.0 for mismatched or zero vectors.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// Deterministic bag-of-words feature-hashing embedder. **Tests and fixtures
/// only**: it is not a semantic model and never ships.
pub struct HashEmbedder {
    dims: usize,
}

impl HashEmbedder {
    pub fn new(dims: usize) -> Self {
        Self { dims: dims.max(1) }
    }

    fn fnv1a(s: &str) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in s.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

impl Embedder for HashEmbedder {
    fn model_id(&self) -> &str {
        "test-hash@0.0.0"
    }

    fn dims(&self) -> usize {
        self.dims
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|t| {
                let mut v = vec![0.0f32; self.dims];
                for w in t
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|w| !w.is_empty())
                {
                    let idx = (Self::fnv1a(&w.to_lowercase()) % self.dims as u64) as usize;
                    v[idx] += 1.0;
                }
                let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                if n > 0.0 {
                    v.iter_mut().for_each(|x| *x /= n);
                }
                v
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_basics() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert_eq!(cosine(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
    }

    #[test]
    fn hash_embedder_is_deterministic_and_similar_for_shared_words() {
        let e = HashEmbedder::new(64);
        let v = e.embed(&["wifi password", "wifi password", "dentist bill"]).unwrap();
        assert_eq!(v[0], v[1]);
        assert!(cosine(&v[0], &v[1]) > cosine(&v[0], &v[2]));
    }
}
