//! Shouxin 示例程序
//!
//! 演示三库分离架构的使用

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Shouxin 三库分离架构示例 ===\n");

    println!("系统已配置为使用三库分离架构");
    println!("包含以下核心组件:");
    println!("  - 字典库 (sled): 永久全量存档");
    println!("  - 关系库 (SQLite): 轻量快速索引");
    println!("  - 向量库: 语义搜索");
    println!("  - 四要素提取: 人物、事件、对话、学习素材");
    println!("  - 待办事项管理");

    println!("\n配置文件支持以下路径设置:");
    println!("  - dictionary_path: 字典库存储路径");
    println!("  - relational_path: 关系库存储路径");
    println!("  - vector_path: 向量库存储路径");
    println!("  - data_dir: 默认数据目录");

    println!("\n使用示例:");
    println!("  1. 创建 ThreeLibraryMemoryManager 实例");
    println!("  2. 调用 add() 添加对话");
    println!("  3. 使用 search_contacts() 等方法搜索");
    println!("  4. 使用 add_todo() 管理待办事项");

    Ok(())
}
