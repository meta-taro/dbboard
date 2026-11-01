-- Fictional shop data for the README screenshots: three databases on one
-- Postgres server, so the sidebar shows databases, schemas and tables.
-- Run as a superuser against a throwaway local server:
--   psql -h 127.0.0.1 -p <port> -U postgres -f scripts/demo-profile/shop-postgres.sql
-- Deterministic: every value comes from generate_series, nothing is random.

CREATE ROLE app LOGIN PASSWORD 'app-local-test';
CREATE DATABASE shop OWNER app;
CREATE DATABASE crm OWNER app;
CREATE DATABASE analytics OWNER app;

\connect crm app
CREATE TABLE leads (id serial PRIMARY KEY, company text, stage text, created_on date);
CREATE TABLE contacts (id serial PRIMARY KEY, lead_id int REFERENCES leads(id), name text);

\connect analytics app
CREATE TABLE daily_sales (day date PRIMARY KEY, orders int, revenue_cents bigint);
CREATE TABLE page_views (id bigserial PRIMARY KEY, path text, viewed_at timestamptz);

\connect shop app
CREATE TABLE products (
  id serial PRIMARY KEY, sku text NOT NULL UNIQUE, name text NOT NULL,
  price_cents int NOT NULL, stock int NOT NULL DEFAULT 0
);
CREATE TABLE customers (id serial PRIMARY KEY, name text NOT NULL, city text NOT NULL, joined_on date NOT NULL);
CREATE TABLE orders (
  id serial PRIMARY KEY,
  customer_id int NOT NULL REFERENCES customers(id),
  product_id int NOT NULL REFERENCES products(id),
  quantity int NOT NULL, ordered_at timestamptz NOT NULL
);
CREATE SCHEMA billing;
CREATE TABLE billing.invoices (
  id serial PRIMARY KEY, order_id int NOT NULL REFERENCES public.orders(id),
  amount_cents int NOT NULL, paid boolean NOT NULL DEFAULT false, issued_on date NOT NULL
);

INSERT INTO products (sku, name, price_cents, stock)
SELECT 'SKU-' || lpad(g::text, 4, '0'),
       (ARRAY['Ceramic mug','Linen tote','Desk lamp','Notebook A5','Fountain pen',
              'Tea sampler','Wool scarf','Plant pot','Wall clock','Coaster set'])[g],
       (ARRAY[1800,2400,6900,900,4500,3200,5800,2100,7400,1500])[g],
       (g * 7) % 40
FROM generate_series(1, 10) g;

INSERT INTO customers (name, city, joined_on)
SELECT (ARRAY['Aoi','Ben','Chika','Dario','Emi','Felix','Gin','Hana','Ivan','Jun','Kai','Lena'])[1 + g % 12]
       || ' ' || (ARRAY['Mori','Hart','Sato','Lind','Okada','Reyes'])[1 + g % 6],
       (ARRAY['Sapporo','Sendai','Nagoya','Kobe','Fukuoka','Naha'])[1 + g % 6],
       date '2025-01-01' + g * 5
FROM generate_series(1, 40) g;

INSERT INTO orders (customer_id, product_id, quantity, ordered_at)
SELECT 1 + (g * 7) % 40, 1 + (g * 3) % 10, 1 + g % 4,
       timestamptz '2026-06-01 09:00+09' + (g * 5 || ' hours')::interval
FROM generate_series(1, 180) g;

INSERT INTO billing.invoices (order_id, amount_cents, paid, issued_on)
SELECT o.id, o.quantity * p.price_cents, o.id % 5 <> 0, o.ordered_at::date
FROM orders o JOIN products p ON p.id = o.product_id;
