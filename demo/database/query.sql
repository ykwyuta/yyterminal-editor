SELECT c.name,o.amount FROM customers c JOIN orders o ON o.customer_id=c.id ORDER BY o.amount;
