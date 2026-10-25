# Database demo

Open Database > DB Manager and connect to SQLite / memory. Open `sqlite.sql` in the normal editor and run it from SQL > DB Execute. Open `query.sql` next to obtain its estimated plan. A fresh in-memory connection is required if you rerun the CREATE statements.

Open any `.yyplan` or `.sqlplan` fixture using Plan > DB Open plan, or `yyte --open-plan PATH`. The seven database fixtures reuse yydbmanager's plan samples. They do not require access to a database server and opening them never executes SQL.

The result grid keeps exact 64-bit integers and uses bounded pages. See [database help](../../docs/user/database.md) for import/export, plan formats, setup, ER and limitations.
