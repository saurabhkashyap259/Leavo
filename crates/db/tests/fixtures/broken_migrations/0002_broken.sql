-- creates a table, then fails: the whole migration must roll back
CREATE TABLE fixture_partial (id int);
SELECT 1 / 0;
