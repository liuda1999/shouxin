use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::core::Memory;
use crate::error::ShouxinError;
use crate::stores::dictionary::SledDictionaryStore;
use crate::stores::relational::FileStore;
use crate::traits::embedder::Embedder;
use crate::traits::vector_store::VectorStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTextDoc {
    pub full_text: String,
    pub chunks: Vec<FileChunk>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChunk {
    pub index: usize,
    pub content: String,
    pub embedding_id: Option<String>,
}

pub struct FileProcessor {
    file_store: FileStore,
    dictionary: Arc<SledDictionaryStore>,
    vector_store: Arc<dyn VectorStore>,
    embedder: Arc<dyn Embedder>,
}

impl FileProcessor {
    pub fn new(
        file_store: FileStore,
        dictionary: Arc<SledDictionaryStore>,
        vector_store: Arc<dyn VectorStore>,
        embedder: Arc<dyn Embedder>,
    ) -> Self {
        Self {
            file_store,
            dictionary,
            vector_store,
            embedder,
        }
    }

    pub async fn process_file(&self, file_id: &str) -> Result<(), ShouxinError> {
        println!("[FileProcessor] Starting processing for file: {}", file_id);

        self.file_store
            .update_file_status(file_id, "processing", None, None)
            .await?;

        let file_record = match self.file_store.get_file(file_id).await? {
            Some(r) => r,
            None => {
                self.file_store
                    .update_file_status(file_id, "failed", Some("文件记录不存在"), None)
                    .await?;
                return Err(ShouxinError::GeneralError("文件记录不存在".to_string()));
            }
        };

        let Some(original_path) = &file_record.original_path else {
            self.file_store
                .update_file_status(file_id, "failed", Some("文件路径不存在"), None)
                .await?;
            return Err(ShouxinError::GeneralError("文件路径不存在".to_string()));
        };

        println!("[FileProcessor] Extracting text from: {} (type: {})", original_path, file_record.file_type);
        let full_text = match Self::extract_text(original_path, &file_record.file_type).await {
            Ok(text) => text,
            Err(e) => {
                let msg = format!("文本提取失败: {}", e);
                eprintln!("[FileProcessor] {} {}", file_id, msg);
                self.file_store
                    .update_file_status(file_id, "failed", Some(&msg), None)
                    .await?;
                return Ok(());
            }
        };

        if full_text.is_empty() {
            self.file_store
                .update_file_status(file_id, "failed", Some("提取文本为空"), None)
                .await?;
            return Ok(());
        }

        println!("[FileProcessor] Extracted {} chars, chunking...", full_text.len());

        let summary = full_text.chars().take(200).collect::<String>();

        let chunks = Self::chunk_text(&full_text);
        let owner_type = file_record.owner_type.clone();
        let owner_id = file_record.owner_id.clone();

        println!("[FileProcessor] Created {} chunks, embedding...", chunks.len());

        let mut chunk_records = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            let embedding_id = format!("file_{}_chunk_{}", file_id, i);

            println!("[FileProcessor] Embedding chunk {}/{} ({} chars)...", i + 1, chunks.len(), chunk.content.len());
            let embedding = self.embedder.embed(&chunk.content).await?;

            let memory = Memory::new(&chunk.content)
                .map_err(|e| ShouxinError::GeneralError(e.to_string()))?
                .with_embedding(embedding)
                .with_metadata("material_type", serde_json::Value::String("file_chunk".into()))
                .with_metadata("source_file_id", serde_json::Value::String(file_id.to_string()))
                .with_metadata("chunk_index", serde_json::Value::String(i.to_string()))
                .with_metadata("file_name", serde_json::Value::String(file_record.file_name.clone()))
                .with_metadata("owner_type", serde_json::Value::String(owner_type.clone()))
                .with_metadata("owner_id", serde_json::Value::String(owner_id.clone()));

            self.vector_store.insert(memory).await?;

            chunk_records.push(FileChunk {
                index: i,
                content: chunk.content.clone(),
                embedding_id: Some(embedding_id),
            });
        }

        let doc = FileTextDoc {
            full_text,
            chunks: chunk_records,
            metadata: serde_json::json!({
                "file_name": file_record.file_name,
                "file_type": file_record.file_type,
            }),
        };

        let json = serde_json::to_vec(&doc)?;
        self.dictionary.save_file_text(file_id, &json)?;

        self.file_store
            .update_file_status(file_id, "completed", None, Some(&summary))
            .await?;

        let _ = std::fs::remove_file(original_path);
        println!("[FileProcessor] File {} completed, {} chunks stored", file_id, chunks.len());

        Ok(())
    }

    pub fn detect_file_type(path: &str) -> String {
        let lower = path.to_lowercase();
        if lower.ends_with(".pdf") {
            "pdf".to_string()
        } else if lower.ends_with(".docx") {
            "doc".to_string()
        } else if lower.ends_with(".xlsx") {
            "xls".to_string()
        } else if lower.ends_with(".csv") {
            "csv".to_string()
        } else if lower.ends_with(".md") || lower.ends_with(".markdown") {
            "md".to_string()
        } else if lower.ends_with(".rs") || lower.ends_with(".py") || lower.ends_with(".js")
            || lower.ends_with(".ts") || lower.ends_with(".go") || lower.ends_with(".java")
            || lower.ends_with(".c") || lower.ends_with(".cpp") || lower.ends_with(".h")
            || lower.ends_with(".html") || lower.ends_with(".css") || lower.ends_with(".json")
            || lower.ends_with(".yaml") || lower.ends_with(".yml") || lower.ends_with(".toml")
        {
            "code".to_string()
        } else if lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg")
            || lower.ends_with(".bmp") || lower.ends_with(".gif") || lower.ends_with(".webp")
        {
            "image".to_string()
        } else {
            "txt".to_string()
        }
    }

    async fn extract_text(path: &str, file_type: &str) -> Result<String, ShouxinError> {
        match file_type {
            "txt" | "md" | "code" => {
                std::fs::read_to_string(path)
                    .map_err(|e| ShouxinError::GeneralError(format!("读取文件失败: {}", e)))
            }
            "pdf" => {
                let bytes = std::fs::read(path)
                    .map_err(|e| ShouxinError::GeneralError(format!("读取PDF文件失败: {}", e)))?;
                pdf_extract::extract_text_from_mem(&bytes)
                    .map_err(|e| ShouxinError::GeneralError(format!("PDF文本提取失败: {}", e)))
            }
            "doc" => {
                let bytes = std::fs::read(path)
                    .map_err(|e| ShouxinError::GeneralError(format!("读取Word文件失败: {}", e)))?;
                Self::extract_docx(&bytes)
            }
            "xls" => {
                Self::extract_xlsx(path)
            }
            "csv" => {
                let content = std::fs::read_to_string(path)
                    .map_err(|e| ShouxinError::GeneralError(format!("读取CSV文件失败: {}", e)))?;
                Self::extract_csv(&content)
            }
            "image" => Err(ShouxinError::GeneralError(
                "图片OCR支持需要安装tesseract系统依赖。请将图片转换为PDF后上传。".to_string(),
            )),
            _ => Err(ShouxinError::GeneralError(
                format!("不支持的文件类型: {}", file_type),
            )),
        }
    }

    fn extract_docx(data: &[u8]) -> Result<String, ShouxinError> {
        let cursor = std::io::Cursor::new(data);
        let mut archive = zip::ZipArchive::new(cursor)
            .map_err(|e| ShouxinError::GeneralError(format!("Word文档ZIP解析失败: {}", e)))?;

        let mut xml_content = String::new();
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)
                .map_err(|e| ShouxinError::GeneralError(format!("读取ZIP条目失败: {}", e)))?;
            if file.name() == "word/document.xml" {
                let mut buf = Vec::new();
                use std::io::Read;
                file.read_to_end(&mut buf)
                    .map_err(|e| ShouxinError::GeneralError(format!("读取document.xml失败: {}", e)))?;
                xml_content = String::from_utf8_lossy(&buf).to_string();
                break;
            }
        }

        if xml_content.is_empty() {
            return Err(ShouxinError::GeneralError("Word文档中未找到document.xml".to_string()));
        }

        let text = Self::strip_xml_tags(&xml_content);
        Ok(text)
    }

    fn strip_xml_tags(xml: &str) -> String {
        let mut result = String::new();
        let mut in_tag = false;
        let mut in_entity = false;
        let mut entity_buf = String::new();

        for ch in xml.chars() {
            if in_entity {
                if ch == ';' {
                    in_entity = false;
                    entity_buf.clear();
                } else {
                    entity_buf.push(ch);
                }
                continue;
            }
            if ch == '<' {
                in_tag = true;
                if !result.is_empty() && !result.ends_with('\n') {
                    result.push('\n');
                }
            } else if ch == '>' {
                in_tag = false;
            } else if !in_tag {
                if ch == '&' {
                    in_entity = true;
                } else {
                    result.push(ch);
                }
            }
        }

        let text = result
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        text
    }

    fn extract_xlsx(path: &str) -> Result<String, ShouxinError> {
        let data = std::fs::read(path)
            .map_err(|e| ShouxinError::GeneralError(format!("读取Excel文件失败: {}", e)))?;
        let cursor = std::io::Cursor::new(data.clone());
        let mut archive = zip::ZipArchive::new(cursor)
            .map_err(|e| ShouxinError::GeneralError(format!("Excel文件ZIP解析失败: {}", e)))?;

        let mut result = String::new();
        let mut sheet_files: Vec<String> = Vec::new();

        for i in 0..archive.len() {
            let file = archive.by_index(i)
                .map_err(|e| ShouxinError::GeneralError(format!("读取ZIP条目失败: {}", e)))?;
            let name = file.name().to_string();
            if name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml") {
                sheet_files.push(name);
            }
        }

        if sheet_files.is_empty() {
            return Err(ShouxinError::GeneralError("Excel文件中未找到工作表".to_string()));
        }

        let mut archive2 = zip::ZipArchive::new(std::io::Cursor::new(data.clone()))
            .map_err(|e| ShouxinError::GeneralError(format!("Excel二次读取失败: {}", e)))?;

        for (idx, sheet_file) in sheet_files.iter().enumerate() {
            if idx > 0 {
                result.push_str("\n\n");
            }
            result.push_str(&format!("=== Sheet {} ===\n", idx + 1));

            let mut file = archive2.by_name(sheet_file)
                .map_err(|e| ShouxinError::GeneralError(format!("读取工作表失败: {}", e)))?;

            let mut buf = Vec::new();
            use std::io::Read;
            file.read_to_end(&mut buf)
                .map_err(|e| ShouxinError::GeneralError(format!("读取工作表内容失败: {}", e)))?;
            let xml = String::from_utf8_lossy(&buf);

            let text = Self::extract_xlsx_rows(&xml);
            result.push_str(&text);
        }

        Ok(result)
    }

    fn extract_xlsx_rows(xml: &str) -> String {
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut current_row: Vec<String> = Vec::new();
        let mut in_cell = false;
        let mut cell_content = String::new();
        let mut tag_buf = String::new();
        let mut in_tag = false;

        for ch in xml.chars() {
            if ch == '<' {
                in_tag = true;
                if !cell_content.is_empty() {
                    current_row.push(cell_content.clone());
                    cell_content.clear();
                }
                tag_buf.clear();
            } else if ch == '>' {
                in_tag = false;
                let tag = tag_buf.trim().to_lowercase();
                if tag == "row" || tag.starts_with("row ") {
                } else if tag == "/row" {
                    if !current_row.is_empty() {
                        rows.push(std::mem::take(&mut current_row));
                    }
                } else if tag == "c" || tag.starts_with("c ") || tag == "v" || tag.starts_with("v ") {
                    in_cell = true;
                    cell_content.clear();
                } else if tag == "/v" || tag == "/c" {
                    in_cell = false;
                }
            } else if in_tag {
                tag_buf.push(ch);
            } else if in_cell {
                cell_content.push(ch);
            }
        }

        let mut result = String::new();
        for row in &rows {
            result.push_str(&row.join("\t"));
            result.push('\n');
        }
        result
    }

    fn extract_csv(content: &str) -> Result<String, ShouxinError> {
        let text = content
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        Ok(text)
    }

    fn chunk_text(text: &str) -> Vec<FileChunk> {
        let paragraphs: Vec<&str> = text.split("\n\n").collect();
        let mut chunks = Vec::new();
        let mut current = String::new();
        let mut chunk_idx = 0;

        for para in paragraphs {
            let trimmed = para.trim();
            if trimmed.is_empty() {
                continue;
            }

            if current.len() + trimmed.len() > 2000 && !current.is_empty() {
                chunks.push(FileChunk {
                    index: chunk_idx,
                    content: std::mem::take(&mut current),
                    embedding_id: None,
                });
                chunk_idx += 1;
            }

            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(trimmed);
        }

        if !current.is_empty() {
            chunks.push(FileChunk {
                index: chunk_idx,
                content: current,
                embedding_id: None,
            });
        }

        chunks
    }
}