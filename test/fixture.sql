CREATE TABLE accounts (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  email text NOT NULL UNIQUE,
  display_name text,
  balance numeric(12,2) NOT NULL DEFAULT 0,
  created_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT accounts_balance_check CHECK (balance >= 0)
);
CREATE INDEX accounts_created_at_idx ON accounts (created_at DESC);
COMMENT ON TABLE accounts IS 'Reorder integration fixture';
COMMENT ON COLUMN accounts.email IS 'Login email';
INSERT INTO accounts (email, display_name, balance) OVERRIDING SYSTEM VALUE VALUES
  ('a@example.com', 'Alpha', 10.50),
  ('b@example.com', 'Beta', 20.00);

CREATE TABLE blocked_generated (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  first_name text NOT NULL,
  last_name text NOT NULL,
  full_name text GENERATED ALWAYS AS (first_name || ' ' || last_name) STORED
);

CREATE TABLE users (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY
);

CREATE TABLE agent_skills (
  id bigserial CONSTRAINT agent_skills_id_not_null NOT NULL,
  user_id bigint CONSTRAINT agent_skills_user_id_not_null NOT NULL,
  name text CONSTRAINT agent_skills_name_not_null NOT NULL,
  alias text CONSTRAINT agent_skills_alias_not_null NOT NULL,
  CONSTRAINT agent_skills_pkey PRIMARY KEY (id),
  CONSTRAINT agent_skills_user_id_foreign FOREIGN KEY (user_id) REFERENCES users(id)
);

CREATE TABLE agent_skillables (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  agent_skill_id bigint NOT NULL,
  CONSTRAINT agent_skillables_agent_skill_id_foreign FOREIGN KEY (agent_skill_id) REFERENCES agent_skills(id)
);

INSERT INTO users DEFAULT VALUES;
INSERT INTO agent_skills (user_id, name, alias) VALUES (1, 'PostgreSQL', 'postgresql');
INSERT INTO agent_skillables (agent_skill_id) VALUES (1);

CREATE TABLE rename_targets (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  code text NOT NULL UNIQUE,
  quantity integer NOT NULL,
  CONSTRAINT rename_targets_quantity_check CHECK (quantity >= 0)
);
CREATE INDEX rename_targets_code_idx ON rename_targets (code);
CREATE TABLE rename_references (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  target_code text NOT NULL REFERENCES rename_targets(code)
);
CREATE VIEW rename_target_view AS SELECT id, code, quantity FROM rename_targets;
INSERT INTO rename_targets (code, quantity) VALUES ('alpha', 3);
INSERT INTO rename_references (target_code) VALUES ('alpha');

CREATE TABLE blocked_options (
  id bigint,
  name text
) WITH (fillfactor = 80);

CREATE TABLE blocked_statistics (
  first_name text,
  last_name text
);
CREATE STATISTICS blocked_statistics_names ON first_name, last_name FROM blocked_statistics;
