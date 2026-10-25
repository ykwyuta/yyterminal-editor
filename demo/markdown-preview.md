# Terminal Markdown preview

This file works in yyte (F5) and markdownViewer.

![Pixel house](preview-pixels.png)

## Flowchart

```mermaid
flowchart LR
    Start[開始] --> Check{検証}
    Check -->|成功| Save[保存]
    Check -->|修正| Start
```

## Sequence

```mermaid
sequenceDiagram
    participant User as User
    participant App as yyte
    User->>App: Open Markdown
    App-->>User: Pixel image and ASCII diagram
```

## Entity relationships

```mermaid
erDiagram
    CUSTOMER ||--o{ ORDER : places
    CUSTOMER {
        int id PK
        string name
    }
    ORDER {
        int id PK
        int customer_id FK
    }
```

## State

```mermaid
stateDiagram-v2
    [*] --> Editing
    Editing --> Preview : F5
    Preview --> Editing : F2
```

- [x] Local images
- [x] Text diagrams
- [ ] Exact browser layout (not required)


## Spreadsheet-style table

| Item / 項目 | Quantity / 数量 | Status / 状態 | Notes / 備考 |
| :--- | ---: | :---: | :--- |
| **顧客一覧** | 1,234 | Ready | Japanese text wraps inside the cell; every column stays aligned. |
| `records.dat` | 42 | Review | Fixed-length records<br>with packed decimal fields |
| [Markdown guide](../docs/user/help.md) | 100000 | Done | Read-only preview with column letters and row numbers. |
