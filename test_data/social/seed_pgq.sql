-- ============================================================
-- Voyager OGM Test Dataset: Social Network
-- Dialect: SQL:2023 PGQ (Property Graph Queries / DuckPGQ)
-- ============================================================

-- 1. Base Relational Tables
CREATE TABLE IF NOT EXISTS social_persons (
    id VARCHAR(64) PRIMARY KEY,
    first_name VARCHAR(64) NOT NULL,
    last_name VARCHAR(64) NOT NULL,
    gender VARCHAR(8) NOT NULL,
    birthday VARCHAR(32) NOT NULL,
    email VARCHAR(128) NOT NULL UNIQUE,
    location_ip VARCHAR(64) NOT NULL
);

CREATE TABLE IF NOT EXISTS social_posts (
    id VARCHAR(64) PRIMARY KEY,
    content TEXT NOT NULL,
    length INT NOT NULL,
    browser_used VARCHAR(64),
    creation_date BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS social_tags (
    id VARCHAR(64) PRIMARY KEY,
    name VARCHAR(64) NOT NULL UNIQUE,
    url TEXT
);

CREATE TABLE IF NOT EXISTS social_knows (
    person1_id VARCHAR(64) NOT NULL REFERENCES social_persons(id),
    person2_id VARCHAR(64) NOT NULL REFERENCES social_persons(id),
    creation_date BIGINT NOT NULL,
    closeness_score DOUBLE PRECISION NOT NULL,
    PRIMARY KEY (person1_id, person2_id)
);

CREATE TABLE IF NOT EXISTS social_creator_of (
    person_id VARCHAR(64) NOT NULL REFERENCES social_persons(id),
    post_id VARCHAR(64) NOT NULL REFERENCES social_posts(id),
    creation_date BIGINT NOT NULL,
    PRIMARY KEY (person_id, post_id)
);

CREATE TABLE IF NOT EXISTS social_has_tag (
    post_id VARCHAR(64) NOT NULL REFERENCES social_posts(id),
    tag_id VARCHAR(64) NOT NULL REFERENCES social_tags(id),
    PRIMARY KEY (post_id, tag_id)
);

CREATE TABLE IF NOT EXISTS social_likes (
    person_id VARCHAR(64) NOT NULL REFERENCES social_persons(id),
    post_id VARCHAR(64) NOT NULL REFERENCES social_posts(id),
    creation_date BIGINT NOT NULL,
    PRIMARY KEY (person_id, post_id)
);

-- 2. ISO SQL:2023 Property Graph Definition
CREATE PROPERTY GRAPH IF NOT EXISTS social_graph
  VERTEX TABLES (
    social_persons
      KEY (id)
      LABEL Person
      PROPERTIES (id, first_name, last_name, gender, birthday, email, location_ip),
    social_posts
      KEY (id)
      LABEL Post
      PROPERTIES (id, content, length, browser_used, creation_date),
    social_tags
      KEY (id)
      LABEL Tag
      PROPERTIES (id, name, url)
  )
  EDGE TABLES (
    social_knows
      KEY (person1_id, person2_id)
      SOURCE KEY (person1_id) REFERENCES social_persons (id)
      DESTINATION KEY (person2_id) REFERENCES social_persons (id)
      LABEL KNOWS
      PROPERTIES (creation_date, closeness_score),
    social_creator_of
      KEY (person_id, post_id)
      SOURCE KEY (person_id) REFERENCES social_persons (id)
      DESTINATION KEY (post_id) REFERENCES social_posts (id)
      LABEL CREATOR_OF
      PROPERTIES (creation_date),
    social_has_tag
      KEY (post_id, tag_id)
      SOURCE KEY (post_id) REFERENCES social_posts (id)
      DESTINATION KEY (tag_id) REFERENCES social_tags (id)
      LABEL HAS_TAG,
    social_likes
      KEY (person_id, post_id)
      SOURCE KEY (person_id) REFERENCES social_persons (id)
      DESTINATION KEY (post_id) REFERENCES social_posts (id)
      LABEL LIKES
      PROPERTIES (creation_date)
  );

-- 3. Data Ingestion
INSERT INTO social_persons (id, first_name, last_name, gender, birthday, email, location_ip) VALUES
  ('usr_001', 'Ada', 'Lovelace', 'F', '1815-12-10', 'ada@example.com', '192.168.1.10'),
  ('usr_002', 'Alan', 'Turing', 'M', '1912-06-23', 'alan@example.com', '192.168.1.11'),
  ('usr_003', 'Grace', 'Hopper', 'F', '1906-12-09', 'grace@example.com', '192.168.1.12'),
  ('usr_004', 'Claude', 'Shannon', 'M', '1916-04-30', 'claude@example.com', '192.168.1.13'),
  ('usr_005', 'John', 'von Neumann', 'M', '1903-12-28', 'john@example.com', '192.168.1.14'),
  ('usr_006', 'Margaret', 'Hamilton', 'F', '1936-08-17', 'margaret@example.com', '192.168.1.15');

INSERT INTO social_tags (id, name, url) VALUES
  ('tag_ai', 'Artificial Intelligence', 'https://topics/ai'),
  ('tag_comp', 'Compiler Engineering', 'https://topics/compilers'),
  ('tag_graph', 'Graph Databases', 'https://topics/graphs'),
  ('tag_info', 'Information Theory', 'https://topics/info_theory');

INSERT INTO social_posts (id, content, length, browser_used, creation_date) VALUES
  ('pst_101', 'Computing machinery and intelligence can transform science.', 60, 'Chrome', 1700000100),
  ('pst_102', 'A symbolic analysis of relay and switching circuits.', 53, 'Firefox', 1700000200),
  ('pst_103', 'Compilers bridge human logic and silicon instructions seamlessly.', 67, 'Safari', 1700000300),
  ('pst_104', 'Software engineering got us safely to the moon.', 48, 'Edge', 1700000400);

INSERT INTO social_creator_of (person_id, post_id, creation_date) VALUES
  ('usr_002', 'pst_101', 1700000100),
  ('usr_004', 'pst_102', 1700000200),
  ('usr_003', 'pst_103', 1700000300),
  ('usr_006', 'pst_104', 1700000400);

INSERT INTO social_has_tag (post_id, tag_id) VALUES
  ('pst_101', 'tag_ai'),
  ('pst_101', 'tag_graph'),
  ('pst_102', 'tag_info'),
  ('pst_103', 'tag_comp'),
  ('pst_104', 'tag_comp');

INSERT INTO social_likes (person_id, post_id, creation_date) VALUES
  ('usr_001', 'pst_101', 1700000500),
  ('usr_003', 'pst_101', 1700000550),
  ('usr_004', 'pst_101', 1700000600),
  ('usr_002', 'pst_102', 1700000650),
  ('usr_001', 'pst_103', 1700000700),
  ('usr_006', 'pst_103', 1700000750),
  ('usr_001', 'pst_104', 1700000800),
  ('usr_003', 'pst_104', 1700000850);

INSERT INTO social_knows (person1_id, person2_id, creation_date, closeness_score) VALUES
  ('usr_001', 'usr_002', 1680000000, 0.95),
  ('usr_002', 'usr_001', 1680000000, 0.95),
  ('usr_002', 'usr_003', 1680000100, 0.88),
  ('usr_003', 'usr_002', 1680000100, 0.88),
  ('usr_002', 'usr_005', 1680000200, 0.92),
  ('usr_005', 'usr_002', 1680000200, 0.92),
  ('usr_003', 'usr_006', 1680000300, 0.85),
  ('usr_006', 'usr_003', 1680000300, 0.85),
  ('usr_004', 'usr_005', 1680000400, 0.90),
  ('usr_005', 'usr_004', 1680000400, 0.90),
  ('usr_001', 'usr_006', 1680000500, 0.78),
  ('usr_006', 'usr_001', 1680000500, 0.78);
