//! HNSW 核心功能测试
//!
//! 直接测试 HNSW 向量库的读写和检索功能

use std::collections::HashMap;
use std::sync::Arc;
use shouxin::core::{Embedding, Memory};
use shouxin::stores::vector::hnsw_store::HnswVectorStore;
use shouxin::stores::vector::types::VectorIndexConfig;
use shouxin::traits::VectorStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("===========================================");
    println!("HNSW 索引核心功能测试");
    println!("===========================================\n");

    println!("步骤 1: 初始化 HNSW 向量库...");
    let config = VectorIndexConfig::default();
    let store = HnswVectorStore::new("./test_hnsw_data", 4, config)?;
    println!("✓ HNSW 向量库初始化成功 (维度: 4)\n");

    println!("步骤 2: 写入真实向量数据...");
    println!("-------------------------------------------");

    let test_data = vec![
        ("记忆1", vec![1.0, 0.0, 0.0, 0.0]),
        ("记忆2", vec![0.0, 1.0, 0.0, 0.0]),
        ("记忆3", vec![0.0, 0.0, 1.0, 0.0]),
        ("记忆4", vec![0.0, 0.0, 0.0, 1.0]),
        ("记忆5", vec![0.8, 0.6, 0.0, 0.0]),
        ("记忆6", vec![0.0, 0.9, 0.1, 0.0]),
        ("记忆7", vec![0.7, 0.7, 0.0, 0.0]),
        ("记忆8", vec![0.0, 0.5, 0.5, 0.0]),
    ];

    for (name, vector) in &test_data {
        let mut memory = Memory::new(name.to_string())?;
        memory.embedding = Some(Embedding {
            vector: vector.clone(),
        });
        store.insert(memory).await?;
        println!("写入: {} - {:?}", name, vector);
    }
    println!("✓ 成功写入 {} 条向量数据\n", test_data.len());

    println!("步骤 3: 执行多次向量检索测试...");
    println!("----------------------------------------\n");

    let queries = vec![
        (vec![0.95, 0.05, 0.0, 0.0], "查询1: 接近 [1,0,0,0]"),
        (vec![0.0, 0.85, 0.15, 0.0], "查询2: 接近 [0,1,0,0]"),
        (vec![0.1, 0.1, 0.8, 0.0], "查询3: 接近 [0,0,1,0]"),
        (vec![0.75, 0.65, 0.0, 0.0], "查询4: 接近记忆5/7"),
    ];

    for (query_vec, description) in queries {
        println!("{}", description);
        println!("查询向量: {:?}", query_vec);
        println!("{}", "-".repeat(50));

        let query_embedding = Embedding {
            vector: query_vec.clone(),
        };

        let results = store.search(&query_embedding, 3, None).await?;

        if results.is_empty() {
            println!("⚠ 未找到相关记忆");
        } else {
            println!("找到 {} 个最相似记忆:", results.len());
            for (i, result) in results.iter().enumerate() {
                println!("  {}. [分数: {:.4}] {}", i + 1, result.score, result.memory.content);
            }
        }
        println!();
    }

    println!("步骤 4: 验证数据统计...");
    println!("----------------------------------------");
    let count = store.count(None).await?;
    println!("当前存储的记忆数量: {}", count);

    let list = store.list(None).await?;
    println!("列表查询返回 {} 条记忆", list.len());
    for (i, memory) in list.iter().enumerate() {
        println!("  {}. {}", i + 1, memory.content);
    }

    println!("\n===========================================");
    println!("HNSW 索引核心功能测试完成！");
    println!("===========================================");

    Ok(())
}
