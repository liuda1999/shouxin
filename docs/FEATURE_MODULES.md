# ShouXin Feature Modules Documentation (v0.5.1)

ShouXin is a personal memory and relationship management system built on a three-library architecture (dictionary, relational, vector). This document describes all 14 feature modules, their APIs, internal architecture, and key data structures.

---

## Module 1: Memory Management (记忆管理)

### Purpose

The core module for adding conversation data and searching across contacts, events, history, and semantic content. It extracts structured memory elements (people, events, todos, facts, learnings) from natural language conversations via LLM and dispatches them to the three-library storage system.

### API Endpoints

#### POST /api/memory/add

Add a conversation for extraction and storage.

**Request (AddConversationRequest):**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| conversation | string | Yes | The conversation text content |
| user_id | string | No | User ID for scope isolation |
| scope | string | No | Operation scope: `User` or `Agent` (default: `User`) |

**Response (AddConversationResponse):**

```json
{
  "success": true,
  "data": {
    "contacts": [],
    "events": [],
    "learnings": [],
    "conversation_id": "550e8400-e29b-41d4-a716-446655440000"
  }
}
```

In v0.5.1, this endpoint uses `add_async`: the conversation is stored immediately, and LLM extraction runs in the background via `BackgroundTaskQueue`. The response returns immediately with the `conversation_id`.

#### GET /api/memory/extract_status/:conversation_id

Get the extraction status for a conversation.

**Response:**

```json
{
  "success": true,
  "data": {
    "conversation_id": "550e8400-...",
    "status": "extracted",
    "extraction_error": null,
    "created_at": "2026-06-05T10:00:00Z",
    "content_preview": "First 200 characters of the conversation..."
  }
}
```

Status values: `pending`, `extracting`, `extracted`, `failed`.

#### POST /api/memory/search/contacts

Search contacts by name or keyword.

**Request (SearchContactsRequest):**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| query | string | Yes | Search query text |
| limit | number | No | Result limit (default: 10) |
| scope | string | No | `user` or `agent` (default: `user`) |
| user_id | string | No | User ID for scope isolation |
| include_archived | boolean | No | Include archived (hot-cold separated) contacts |

**Response:** Array of `ContactDetail` objects with `contact_id`, `name`, `aliases`, `background`, `last_interaction_at`, `activity_score`, `related_contacts`, `relationship_tags`.

#### POST /api/memory/search/events

Search events with flexible filtering.

**Request (SearchEventsRequest):**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| user_id | string | No | User ID |
| event_type | string | No | Event type filter |
| start_time | string | No | RFC3339 start time |
| end_time | string | No | RFC3339 end time |
| importance | string | No | `high`, `medium`, `low`, `short_term`, `long_term`, `critical` |
| status | string | No | `pending`, `completed` |
| limit | number | No | Result limit (default: 10) |
| scope | string | No | Operation scope |
| include_archived | boolean | No | Include archived events |
| archived_month | string | No | Filter by archived month (e.g. "2026-06") |

#### POST /api/memory/search/history

Search conversation history by time range.

**Request (SearchHistoryRequest):**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| start_time | string | No | RFC3339 start time (default: epoch) |
| end_time | string | No | RFC3339 end time (default: now) |
| scope | string | No | Operation scope |
| user_id | string | No | User ID |

#### POST /api/memory/search/semantic

Semantic (vector) search across learning materials.

**Request (SemanticSearchRequest):**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| query | string | Yes | Search query text |
| limit | number | No | Result limit (default: 10) |
| scope | string | No | Operation scope |
| user_id | string | No | User ID |
| source_types | string[] | No | Filter by source type (e.g. `["event", "learning"]`) |
| conversation_id | string | No | Filter by specific conversation (v0.5.1) |

**Response:** Array of `VectorSearchResult` with `material` (LearningMaterial) and `score` (cosine similarity, 0-1).

### Internal Architecture

1. **Extraction Pipeline**: `LlmMemoryExtractor` processes conversation text using LLM with structured JSON output prompts. The extractor uses a comprehensive system prompt with few-shot examples for Chinese-language person and event extraction.

2. **LLM Output Sanitizer (`LlmOutputSanitizer`)**: Handles garbled/output issues from GGUF models:
   - Character set detection: detects Cyrillic characters and low Chinese ratio as garbled output
   - JSON fragment extraction: extracts the first `{...}` block from garbled output
   - Heuristic intent extraction: uses regex to extract todo update intents from conversation text when LLM fails

3. **DataDispatcher**: Routes extracted elements to three libraries:
   - Dictionary (Sled): full conversation text, contact totals, contact event history
   - Relational (SQLite/WAL): contact index, event index, todo items
   - Vector (HNSW/Embedded): learning materials and event embeddings

4. **Async Queue (v0.5.1)**: `BackgroundTaskQueue` enables non-blocking extraction. Conversations are persisted immediately; extraction tasks are queued and processed by a worker thread.

5. **Fallback Extraction**: When LLM extraction fails, the system falls back to regex-based extraction using Chinese surname patterns, title patterns, and event keyword matching.

---

## Module 2: Todo Management (待办管理)

### Purpose

Manages todo items with CRUD operations, archive-on-write triggers, reminder generation, and hot-cold separation.

### API Endpoints

#### POST /api/todos

Create a new todo item. Triggers archive-on-write and reminder generation.

**Request (AddTodoRequest):**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| todo_id | string | No | Client-provided ID (server generates UUID if not provided) |
| contact_ids | string[] | No | Related contact IDs |
| title | string | Yes | Todo title (max 255 chars, trimmed) |
| description | string | No | Detailed description |
| deadline | string | Yes | Deadline in RFC3339 format |
| importance | string | No | `high`, `medium`, `low`, `short_term`, `long_term`, `critical` (default: `medium`) |
| source_event_id | string | No | Source event ID reference |
| scope | string | No | Operation scope |
| user_id | string | No | User ID |

**Validation:**
- Title is trimmed and validated for non-empty
- Title truncated to 255 characters (UTF-8 safe)
- Invalid importance values downgraded to `medium`
- Duplicate check: same title + same deadline (1-minute resolution) + same owner = 409 Conflict

**Response:** The created `TodoItem` object.

#### GET /api/todos

List todos with filtering.

**Query Parameters (ListTodosRequest):**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| status | string | No | `pending`, `completed`, `auto_invalid` |
| contact_id | string | No | Filter by contact ID |
| limit | number | No | Result limit (default: 100) |
| scope | string | No | Operation scope |
| user_id | string | No | User ID |
| include_archived | boolean | No | Include archived todos |

**Response:** Array of `TodoItem` objects, sorted by deadline ascending.

#### GET /api/todos/:id

Get a single todo detail.

**Query Parameters (ScopeQueryParams):** `scope`, `user_id` (optional, auto-resolved from todo owner if not provided).

#### PUT /api/todos/:id/complete

Mark a todo as completed. Sets `completed_at` and `last_updated_at` to current time.

#### DELETE /api/todos/:id

Delete a todo. Requires owner scope match.

### Internal Architecture

**Archive-on-Write Trigger**: When `add_todo` is called, the system checks pending todo count:
- If `pending_count > 50`: archives stale todos (not updated for 30+ days), keeping only the most recent 50 pending
- If `pending_count > 30`: generates a reminder todo (type=`reminder`) with title `"⚠️ 你有 N 个待办未处理，请及时处理"`, only if no reminder exists for today

**Reminder Dedup**: Checks for existing reminders created today via `has_recent_reminder()`.

### Key Data Structure: TodoItem

```rust
pub struct TodoItem {
    pub todo_id: String,              // Unique ID
    pub contact_ids: Vec<String>,     // Related contact IDs (JSON array)
    pub owner_id: String,             // Owner ID
    pub owner_type: OwnerType,        // User, Agent, Contact, Public
    pub title: String,                // Title
    pub description: Option<String>,  // Description
    pub deadline: DateTime<Utc>,      // Deadline
    pub status: String,               // pending, completed, auto_invalid
    pub created_at: DateTime<Utc>,    // Creation time
    pub completed_at: Option<DateTime<Utc>>, // Completion time
    pub importance: String,           // high, medium, low, short_term, long_term, critical
    pub source_event_id: Option<String>, // Source event ID
    pub todo_type: String,            // normal, ambiguity, confirmation, reminder
    pub entity_type: Option<String>,  // Related entity type
    pub entity_id: Option<String>,    // Related entity ID
    pub is_archived: bool,            // Hot-cold archive flag
    pub archived_at: Option<DateTime<Utc>>, // Archive time
    pub last_updated_at: DateTime<Utc>, // Last update time
}
```

**Unique constraint**: `(title, owner_id, owner_type, deadline)` WHERE `status='pending'` (enforced via `INSERT OR IGNORE`).

---

## Module 3: Heartbeat System (心跳系统)

### Purpose

A periodic background task system that performs maintenance operations (data cleanup, dedup, merge) and LLM-based analysis (profile updates, question generation). Uses a split architecture to isolate maintenance tasks from LLM tasks.

### API Endpoints

#### POST /api/heartbeat/trigger

Manually trigger a heartbeat execution. Supports both maintenance and LLM tasks.

**Request:** Optional JSON body (not required).

**Response:** `HeartbeatReport` object. Returns 409 Conflict if a heartbeat is already executing.

#### GET /api/heartbeat/pending_questions

Get all pending questions generated by the heartbeat.

**Response:** Array of `Question` objects with `id`, `priority`, `category`, `title`, `detail`, `context`, `related_contact_ids`, `related_event_ids`.

#### GET /api/heartbeat/status

Get current heartbeat system status.

**Response:**

```json
{
  "success": true,
  "data": {
    "is_running": true,
    "last_heartbeat": "2026-06-05T09:00:00Z",
    "last_memory_write": "2026-06-05T10:00:00Z",
    "pending_questions_count": 3,
    "next_heartbeat_in_secs": 360,
    "consecutive_skips": 0,
    "last_skipped": false,
    "available_memory_mb": 8192
  }
}
```

#### POST /api/heartbeat/answer

Answer a pending question.

**Request (AnswerRequest):**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| question_id | string | Yes | Question ID to answer |
| answer | string | Yes | Answer text |

### Internal Architecture

**Split Architecture**:
- **Maintenance Heartbeat** (30s timeout): Pure SQL tasks, never calls LLM. Includes stale contact cleanup, event dedup/cleanup, contact merge, ambiguity detection, todo hot-cold management.
- **LLM Heartbeat** (120s timeout): LLM-based tasks including profile analysis and question generation.

**Scheduler**: `HeartbeatScheduler` runs on a configurable interval (default: 360s). Features:
- Adaptive interval with memory threshold checking (min 4GB free)
- Maximum consecutive skip limit (3) before forcing execution
- Concurrency control: `AtomicBool` prevents overlapping executions

### Key Data Structure: HeartbeatReport

```rust
pub struct HeartbeatReport {
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub actions_generated: usize,
    pub questions_created: usize,
    pub contacts_updated: usize,
    pub events_cleaned: usize,
    pub events_merged: usize,
    pub contacts_merged: usize,
    pub ambiguities_detected: usize,
    pub profiles_updated: usize,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub duration_ms: u64,
    pub maintenance_only: bool,
}
```

### Configuration

```toml
[heartbeat]
enabled = true
base_interval_secs = 360
min_interval_secs = 300
max_interval_secs = 600
adaptive = false
idle_threshold_secs = 300
```

---

## Module 4: File Management (文件管理)

### Purpose

Handles file upload, metadata retrieval, text extraction, and deletion. Uses MD5-based deduplication and async background processing for text extraction.

### API Endpoints

#### POST /api/files/upload

Upload a file for processing. Raw bytes in body, metadata in query parameters.

**Query Parameters:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| filename | string | No | File name (default: `upload.txt`) |
| scope | string | No | Owner scope (default: `agent`) |
| user_id | string | No | User ID (default: `primary_agent`) |
| target_type | string | No | Target entity type for file relation |
| target_id | string | No | Target entity ID for file relation |

**Constraints:**
- Max file size: 20MB
- Supported types: `txt`, `md`, `code`, `pdf`, `doc`, `xls`, `csv`

**MD5 Dedup:** Same file content + same owner returns the existing `file_id`:

```json
{
  "success": true,
  "data": {
    "file_id": "existing-uuid",
    "status": "completed",
    "dedup": true
  }
}
```

**New upload:**

```json
{
  "success": true,
  "data": {
    "file_id": "new-uuid",
    "status": "pending"
  }
}
```

#### GET /api/files/:file_id

Get file metadata. Returns `FileIndex` object.

#### GET /api/files/:file_id/text

Get extracted text content. Returns `{"file_id": "...", "full_text": "..."}`.

#### DELETE /api/files/:file_id

Delete a file and its text. Also deletes file relations.

### Internal Architecture

**Async Processing**: Database insert is synchronous (metadata immediately available), while file text extraction runs asynchronously via `tokio::spawn`:
1. File content saved to `./temp_uploads/<file_id>`
2. Record inserted into `file_index` table with status `pending`
3. Background task spawns `FileProcessor::process_file()` which extracts text, updates status to `completed` or `failed`

**MD5 Dedup**: Unique index on `(md5_hash, owner_type, owner_id)` prevents duplicate uploads.

### Key Data Structure: FileIndex

```rust
pub struct FileIndex {
    pub file_id: String,
    pub file_name: String,
    pub file_type: String,           // txt, md, code, pdf, doc, xls, csv
    pub mime_type: Option<String>,
    pub file_size: Option<i64>,
    pub md5_hash: Option<String>,
    pub owner_type: String,
    pub owner_id: String,
    pub uploaded_at: Option<String>,
    pub processed_at: Option<String>,
    pub status: String,              // pending, completed, failed
    pub error_message: Option<String>,
    pub original_path: Option<String>,
    pub extracted_text_summary: Option<String>,
}
```

---

## Module 5: Virtual Entities (虚拟实体管理)

### Purpose

Manages virtual/fictional characters as contacts with alias support. Characters are stored as `ContactIndex` records with `is_virtual=true` and have associated aliases for name matching.

### API Endpoints

#### GET /api/virtual_entities

List all virtual entities.

**Response:** Array of `VirtualCharacterInfo` with `contact` (ContactIndex) and `aliases` (array of VirtualCharacterAlias).

#### GET /api/virtual_entities/:id

Get a single virtual entity by contact ID.

#### POST /api/virtual_entities

Add a new virtual character.

**Request (AddVirtualCharacterRequest):**

```json
{
  "name": "林克",
  "aliases": [
    {"name": "Link", "source_work": "塞尔达传说", "alias_type": "official"},
    {"name": "时之勇者", "source_work": "塞尔达传说", "alias_type": "nickname"}
  ]
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| name | string | Yes | Character name |
| aliases | AliasInput[] | Yes | List of aliases |

**AliasInput:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| name | string | Yes | Alias name |
| source_work | string | No | Source work (e.g. game, book) |
| alias_type | string | No | Alias type (e.g. `official`, `nickname`, `localized`, `title`) |

#### PUT /api/virtual_entities/:id

Update a virtual entity. Supports partial updates: `name` and `aliases` are both optional. If `aliases` is provided, all existing aliases are replaced.

**Request (UpdateVirtualCharacterRequest):**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| name | string | No | New character name |
| aliases | AliasInput[] | No | New alias list (replaces all) |

#### DELETE /api/virtual_entities/:id

Delete a virtual entity and all its aliases.

#### GET /api/virtual_entities/check/:name

Check if a name matches any virtual entity. Searches both alias names and contact names (case-insensitive).

**Response:** `VirtualCharacterInfo` if found, or error.

### Internal Architecture

- Virtual characters are stored as `ContactIndex` records with `is_virtual=true, owner_id="system"` (built-in) or `owner_id="default_user"` (user-created).
- Aliases are stored in the `virtual_character_aliases` table.
- Built-in virtual characters include: 林克, ChatGPT, 马里奥, 皮卡丘, 孙悟空, 曹操, 蝙蝠侠, 钢铁侠, 克劳德, 哆啦A梦, 柯南.
- Built-in characters are initialized on server startup if not already present.

### Key Data Structures

```rust
pub struct VirtualCharacterAlias {
    pub id: i64,
    pub contact_id: String,
    pub alias_name: String,
    pub source_work: Option<String>,
    pub alias_type: String,
    pub created_at: DateTime<Utc>,
}

pub struct VirtualCharacterInfo {
    pub contact: ContactIndex,
    pub aliases: Vec<VirtualCharacterAlias>,
}
```

---

## Module 6: Event Merge (事件合并管理)

### Purpose

Manages the event merge workflow for semantic deduplication of events. When new events are added, the system checks for semantic similarity with existing events and either auto-merges (similarity > 0.90) or creates a pending merge for manual review (similarity 0.75-0.90).

### API Endpoints

#### POST /api/events/pending_merges

Get pending event merge items that require manual confirmation.

**Request (GetPendingMergesRequest):**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| owner_type | string | No | `User`, `Agent`, `Contact`, `Public` |
| owner_id | string | No | Owner ID |
| limit | number | No | Result limit (default: 10) |

**Response:** Array of `PendingMergeItem`:

```json
{
  "success": true,
  "data": [
    {
      "id": 1,
      "source_event_id": "pending_uuid",
      "target_event_id": "existing_event_uuid",
      "source_event_summary": "New event summary",
      "target_event_summary": "Existing event summary",
      "similarity_score": 0.82,
      "status": "pending",
      "created_at": "2026-06-05T10:00:00Z"
    }
  ]
}
```

#### POST /api/events/confirm_merge

Confirm or reject a pending event merge.

**Request (ConfirmMergeRequest):**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| merge_id | number | Yes | Pending merge record ID |
| action | string | Yes | `merge` (merge into target) or `create` (create as new event) |
| source_event_summary | string | No | Source event summary (required when action=`create`) |

**Response:**

```json
{
  "success": true,
  "data": {
    "success": true,
    "message": "事件已合并",
    "event_id": "merged_event_uuid"
  }
}
```

### Internal Architecture

**Semantic Merge Pipeline** (v0.5.1):
1. When a new event is being added, the system generates an embedding via `FastEmbedder`
2. Searches the vector store for similar events (top 3)
3. If similarity > 0.90: auto-merge into the existing event
4. If similarity 0.75-0.90: create a pending merge record for review
5. If similarity < 0.75: treat as a new event

### Key Data Structure: PendingMergeItem

```rust
pub struct PendingMergeItem {
    pub id: i64,
    pub source_event_id: String,
    pub target_event_id: Option<String>,
    pub source_event_summary: String,
    pub target_event_summary: Option<String>,
    pub similarity_score: f32,
    pub status: String,       // pending, merged, created, dismissed
    pub created_at: String,
}
```

---

## Module 7: Vector Search (向量搜索)

### Purpose

Provides semantic search via vector embeddings. Uses the same underlying endpoint as the memory semantic search.

### API Endpoints

#### POST /api/vector/search

Vector semantic search. Same endpoint as `/api/memory/search/semantic`.

**Request (SemanticSearchRequest):**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| query | string | Yes | Search query text |
| limit | number | No | Result limit (default: 10) |
| scope | string | No | Operation scope |
| user_id | string | No | User ID |
| source_types | string[] | No | Filter by source type |
| conversation_id | string | No | Filter by conversation ID |

#### GET /api/vector/:id

Get learning material detail by ID.

**Response:** `LearningMaterial` object.

### Internal Architecture

- **Embeddings**: 768-dimensional vectors generated by `intfloat/multilingual-e5-base` via FastEmbed
- **Vector Store**: Configurable between embedded (in-memory usearch) and HNSW (disk-persistent) backends
- **Search**: Uses cosine similarity scoring. Results filtered by owner scope and optionally by source type and conversation ID
- **Fallback**: If owner-filtered search returns empty, retries without owner filter

### Key Data Structures

```rust
pub struct LearningMaterial {
    pub id: u64,
    pub content: String,
    pub summary: Option<String>,
    pub source_conversation_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub material_type: String,          // knowledge, fact, preference, event, learning
    pub canonical_entity_id: Option<String>,
    pub keywords: Vec<String>,
    pub context_before_id: Option<String>,
    pub context_after_id: Option<String>,
    pub triples: Vec<KnowledgeTriple>,
    pub owner_type: OwnerType,
    pub owner_id: String,
}

pub struct VectorSearchResult {
    pub material: LearningMaterial,
    pub score: f32,                     // Cosine similarity, 0-1
}
```

---

## Module 8: Smart Search (智能搜索)

### Purpose

Unified search across multiple data sources: contacts, events, history, and todos. Returns aggregated results from all sources in a single response.

### API Endpoints

#### POST /api/search/smart

Unified search across multiple data types.

**Request (SmartSearchRequest):**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| query | string | Yes | Search query text |
| limit | number | No | Per-type result limit (default: 10, max: 20) |
| types | string[] | No | Search types (default: `["contacts", "events", "history", "todos"]`) |
| scope | string | No | Operation scope |
| user_id | string | No | User ID |

**Response (SmartSearchResponse):**

```json
{
  "success": true,
  "data": {
    "contacts": [...],
    "events": [...],
    "history": [...],
    "todos": [...]
  }
}
```

### Internal Architecture

The smart search iterates over requested types and performs separate searches:
- **contacts**: Uses `search_contacts()` with the query string
- **events**: Uses `search_events()` with a search query filter
- **todos**: Fetches all todos, then filters in-memory by title/description substring match
- **history**: Fetches recent 30 days of history, then filters in-memory

Each type is limited to `min(limit, 20)` results to prevent timeouts.

---

## Module 9: Internal API (内部API)

### Purpose

Internal API for sub-agent registration and event creation. Requires Bearer token authentication.

### API Endpoints

#### POST /internal/agent/register

Register a sub-agent.

**Auth:** `Authorization: Bearer {token}` (token from `INTERNAL_API_TOKEN` env var, no default value, returns 500 if INTERNAL_API_TOKEN env var not set)

**Request (AgentRegistrationRequest):**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| agent_id | string | Yes | Agent unique ID |
| name | string | Yes | Agent display name |
| description | string | No | Agent description |

**Response:** 201 Created on success, 409 Conflict if agent already exists.

#### POST /internal/event/add

Add an internal event.

**Auth:** `Authorization: Bearer {token}`

**Request (InternalEventAddRequest):**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| description | string | Yes | Event description |
| related_contacts | string[] | Yes | Related contact IDs |
| source_agent_id | string | Yes | Source agent ID |
| owner_type | string | No | `Agent` or `User` (default: `Agent`) |
| owner_id | string | No | Owner ID (default: `primary_agent`) |

**Response:** 201 Created with `event_id`.

### Internal Architecture

- `InternalTokenAuth` middleware extracts `Authorization: Bearer` header
- Token compared against `INTERNAL_API_TOKEN` env var (no default value, returns 500 if INTERNAL_API_TOKEN env var not set)
- Agent registration creates both `ContactIndex` and `ContactTotal` records with `is_agent=true`
- Internal events are created with UUID, parsed time, and medium importance

---

## Module 10: Admin API (管理API)

### Purpose

Administrative endpoints for LLM backend management and status monitoring.

### API Endpoints

#### GET /admin/llm/status

Get LLM manager status.

**Auth:** `X-Internal-Token` header (token from `config.toml` `[agent] internal_api_token`)

**Response:**

```json
{
  "success": true,
  "data": {
    "active_source": "ollama",
    "is_primary": true,
    "total_requests": 150,
    "total_failures": 3,
    "primary_healthy": true,
    "secondary_healthy": true,
    "auto_switch_enabled": true
  }
}
```

#### POST /admin/llm/switch

Switch LLM backend.

**Auth:** `X-Internal-Token` header

**Request:**

```json
{
  "target": "primary"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| target | string | Yes | `primary` or `secondary` |

**Response:**

```json
{
  "success": true,
  "data": {
    "switched_to": "primary"
  }
}
```

### Configuration

The admin token is configured in `config.toml`:

```toml
[agent]
primary_agent_id = "primary_agent_001"
internal_api_token = "your-secure-admin-token"
```

---

## Module 11: Health Check

### Purpose

Simple health check endpoint for monitoring and load balancer integration.

### API Endpoints

#### GET /health

**Response:**

```json
{
  "status": "ok",
  "time": "2026-06-05T10:00:00.123456789Z"
}
```

No authentication required.

---

## Module 12: LLM System (LLM系统)

### Purpose

Multi-backend LLM client system with primary/secondary failover, auto-switch on failure, and health check monitoring.

### Internal Architecture

**Backend Types:**
- **GGUF** (`llama-cpp-2`): Local model inference, configurable context size, threads, batch size, GPU layers
- **Ollama**: HTTP API to Ollama server, supports model name and endpoint configuration
- **API** (OpenAI-compatible): HTTP API to any OpenAI-compatible endpoint, supports custom headers and API keys

**LlmClientManager:**
- Wraps a primary and optional secondary `LlmClient`
- Uses `AtomicUsize` for lock-free switching between primary/secondary
- Implements `LlmClient` trait via proxy pattern, all requests go through the active client
- Exponential backoff retry: up to 2 attempts with 2^n * 1000ms delay
- Auto-switch: after `max_consecutive_failures` consecutive primary failures, switches to secondary
- Background health check: pings primary every `health_interval_secs`; auto-switches back when primary recovers
- Manual switch via Admin API

**ManagerStatus:**

```rust
pub struct ManagerStatus {
    pub active_source: String,           // "ollama", "gguf", "api"
    pub is_primary: bool,
    pub total_requests: u64,
    pub total_failures: u64,
    pub health_check_interval_secs: u64,
    pub primary_healthy: bool,
    pub secondary_healthy: Option<bool>,
    pub auto_switch_enabled: bool,
}
```

### Configuration

```toml
[llm]
primary = "ollama"           # ollama, gguf, api
secondary = "api"            # Optional backup
temperature = 0.7
max_tokens = 2000
timeout_secs = 60

[llm.ollama]
model = "llama3.2:3b"
endpoint = "http://localhost:11434"

[llm.gguf]
model_path = "models/Llama-3.2-3B-Instruct-Q5_K_M.gguf"
context_size = 2048
threads = 4

[llm.api]
endpoint = "https://api.openai.com/v1"
api_key = "sk-..."
model = "gpt-4o-mini"
```

---

## Module 13: Three-Library Storage (三库存储)

### Purpose

The foundational storage architecture that separates data into three specialized libraries for different access patterns and durability requirements.

### Architecture

**1. Dictionary Library (字典库 - Sled)**
- Storage engine: Sled (embedded key-value store, LSM tree)
- Purpose: Permanent archive of all raw data
- Stores: `ContactTotal` (full contact records), `ConversationRecord` (raw conversation text), `ContactEventRecord` (event history per contact), file extracted text
- Data is permanent and never deleted
- Graceful shutdown: flush + close to prevent lock residue

**2. Relational Library (关系库 - SQLite/WAL)**
- Storage engine: SQLite with WAL journal mode, 64 max connections, 5s busy timeout
- Purpose: Fast index for structured queries
- Stores: `ContactIndex` (lightweight contact index), `EventIndex` (event metadata), `TodoItem`, `FileIndex`, `Conversation` metadata, virtual entity aliases, pending event merges
- Supports hot-cold separation: active records stay in hot tables, stale records archived
- Enforces per-owner isolation via `owner_type` and `owner_id` columns

**3. Vector Library (向量库 - HNSW/Embedded)**
- Storage engine: Configurable between embedded (in-memory usearch) and HNSW (disk-persistent)
- Purpose: High-dimensional semantic search
- Embedding dimension: 768 (intfloat/multilingual-e5-base)
- Distance metric: Cosine similarity
- Stores: `LearningMaterial` embeddings with metadata
- Auto-dedup: skips inserts with similarity > 0.98 to existing materials
- Configurable via `SHOUXIN_STORE_TYPE` environment variable (`embedded` or `hnsw`)

### Key Data Flow

```
Conversation Input
    |
    v
LLM Memory Extractor (LlmMemoryExtractor)
    |
    v
Data Dispatcher (DataDispatcher)
    |
    +---> Dictionary Library (Sled): full text, contact totals, event history
    |
    +---> Relational Library (SQLite): contact index, event index, todos
    |
    +---> Vector Library (HNSW/Embedded): learning material embeddings
```

### Configuration

```toml
[storage]
data_dir = "./data"
dictionary_dir = "dictionary"
relational_dir = "relational"
vector_dir = "vector"
relational_db_file = "relational.db"
vector_index_file = "vectors.hnsw"
```

---

## Module 14: Memory Extractor (记忆提取)

### Purpose

Extracts structured memory elements (people, events, todos, facts, learnings) from natural language conversation text using LLM with prompt engineering, fallback regex extraction, and output sanitization.

### Internal Architecture

**LlmMemoryExtractor:**
- Uses a detailed system prompt with few-shot examples for Chinese-language extraction
- Prompt instructs the LLM to output structured JSON with `people`, `events`, and `todo_update_intent` fields
- Conversation text truncated to 1024 characters before sending to LLM
- Extraction timeout: 600 seconds (sync mode)

**Extraction Pipeline:**
1. LLM extraction with structured JSON output
2. Response parsing: strips markdown code fences, extracts JSON block
3. **LlmOutputSanitizer** checks for garbled output:
   - Character set detection: flags output with > 5% Cyrillic or < 30% Chinese characters
   - JSON fragment extraction: finds `{...}` block in garbled output, replaces Cyrillic with spaces
   - Heuristic todo intent extraction: regex matching for deadline change patterns
4. Fallback extraction when LLM fails:
   - Chinese surname + title pattern matching (e.g. "王经理", "赵总", "小丽")
   - Nickname pattern: "老X", "小X"
   - English name pattern: `[A-Z][a-z]+ [A-Z][a-z]+`
   - Event keyword matching (e.g. "会议", "讨论", "评审", "开发")
   - Compound relation extraction: "X的女儿", "X的秘书"

**Contact Validation:**
- Two-stage validation: rule-based filter then LLM-based verification
- Rule-based filter checks: empty names, single-character names, garbage words, action suffixes, non-person words
- LLM validation: sends extracted names back to LLM for verification ("is this a person name?")
- Name truncation fix: `fix_name_truncation()` removes common word suffixes that were incorrectly appended to names

**Multi-Segment Extraction:**
- For long conversations (> 80 chars), segments the text and extracts persons from each segment
- Merges results with deduplication

**DataDispatcher:**
- Routes extracted elements to three libraries:
  - **Dictionary**: full conversation text, contact totals, contact event history with sentiment analysis
  - **Relational**: contact index with dedup by name+owner, event index with dedup by summary+owner, active contact management (max 50)
  - **Vector**: learning materials with quality threshold (>= 0.8), embedding generation, similarity-based dedup, entity normalization, and summarization

**Extracted Element Types:**

```rust
pub struct ExtractedElements {
    pub conversation_text: String,
    pub people: Vec<ExtractedPerson>,
    pub events: Vec<ExtractedEvent>,
    pub todos: Vec<ExtractedTodo>,
    pub facts: Vec<ExtractedFact>,
    pub learnings: Vec<ExtractedLearning>,
    pub todo_update_intent: Option<TodoUpdateIntent>,
}

pub struct ExtractedPerson {
    pub contact_id: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub role: Option<String>,
    pub description: Option<String>,
    pub contact_type: String,
}

pub struct ExtractedEvent {
    pub event_id: String,
    pub title: Option<String>,
    pub description: String,
    pub time: Option<String>,
    pub time_info: Option<String>,
    pub location: Option<String>,
    pub importance: f32,
    pub importance_label: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub related_person_ids: Vec<String>,
}

pub struct TodoUpdateIntent {
    pub title_hint: Option<String>,
    pub new_deadline: Option<String>,
}
```

**LlmOutputSanitizer Methods:**

| Method | Description |
|--------|-------------|
| `has_garbage_chars(text)` | Detects > 5% Cyrillic or < 30% Chinese in output |
| `extract_json_fragment(text)` | Extracts `{...}` block, replaces Cyrillic with spaces |
| `extract_todo_intent_heuristic(text)` | Regex-based extraction of deadline change intent |

---

## Appendix: Server Configuration

### config.toml Example

```toml
[llm]
primary = "ollama"
secondary = "api"
temperature = 0.7
max_tokens = 2000
timeout_secs = 60

[llm.ollama]
model = "llama3.2:3b"
endpoint = "http://localhost:11434"

[llm.gguf]
model_path = "models/Llama-3.2-3B-Instruct-Q5_K_M.gguf"
context_size = 2048
threads = 4

[llm.api]
endpoint = "https://api.openai.com/v1"
api_key = "sk-..."
model = "gpt-4o-mini"

[agent]
primary_agent_id = "primary_agent_001"
internal_api_token = "your-admin-token"

[heartbeat]
enabled = true
base_interval_secs = 1800
adaptive = true

[storage]
data_dir = "./data"

[logging]
level = "info"
dir = "./logs"
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `SHOUXIN_STORE_TYPE` | Vector store type: `embedded` or `hnsw` |
| `SHOUXIN_CONFIG` | Path to config.toml |
| `FASTEMBED_CACHE_DIR` | FastEmbed model cache directory |
| `INTERNAL_API_TOKEN` | Internal API auth token |
| `SHOUXIN_<SECTION>__<KEY>` | Override any config value (double underscore separator) |

### Server Startup

```
cargo run --release -- --host 127.0.0.1:3000
```

The server starts two main components:
- **API thread**: HTTP server on the specified host, with configurable timeout middleware
- **Background queue worker**: Processes async extraction tasks via `BackgroundTaskQueue`
- **Heartbeat scheduler**: Periodic maintenance and LLM tasks

Graceful shutdown sequence on Ctrl+C: vector store persist, SQLite WAL checkpoint, sled dictionary flush and close.