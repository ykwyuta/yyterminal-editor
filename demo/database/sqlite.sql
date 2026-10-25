-- SQLite / memory: connect, then execute this script.
CREATE TABLE customers(id INTEGER PRIMARY KEY,name TEXT NOT NULL);
CREATE TABLE orders(id INTEGER PRIMARY KEY,customer_id INTEGER REFERENCES customers(id),amount DECIMAL(18,2));
CREATE INDEX orders_customer ON orders(customer_id);
INSERT INTO customers VALUES(1,'東京'),(2,'大阪');
INSERT INTO orders VALUES(1,1,1250.25),(2,1,9876.50),(3,2,42);
SELECT c.name,o.amount FROM customers c JOIN orders o ON o.customer_id=c.id ORDER BY o.amount;
