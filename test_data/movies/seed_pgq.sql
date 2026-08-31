-- ============================================================
-- Voyager OGM Official Canonical Dataset: Neo4j Movie Graph
-- Dialect: SQL:2023 PGQ (Property Graph Queries / DuckPGQ)
-- ============================================================

-- 1. Base Relational Tables
CREATE TABLE IF NOT EXISTS persons (
    id VARCHAR(64) PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    born INT
);

CREATE TABLE IF NOT EXISTS movies (
    id VARCHAR(64) PRIMARY KEY,
    title VARCHAR(255) NOT NULL,
    tagline TEXT,
    released INT NOT NULL
);

CREATE TABLE IF NOT EXISTS acted_in (
    person_id VARCHAR(64) NOT NULL REFERENCES persons(id),
    movie_id VARCHAR(64) NOT NULL REFERENCES movies(id),
    role VARCHAR(255) NOT NULL,
    PRIMARY KEY (person_id, movie_id, role)
);

CREATE TABLE IF NOT EXISTS directed (
    person_id VARCHAR(64) NOT NULL REFERENCES persons(id),
    movie_id VARCHAR(64) NOT NULL REFERENCES movies(id),
    PRIMARY KEY (person_id, movie_id)
);

CREATE TABLE IF NOT EXISTS produced (
    person_id VARCHAR(64) NOT NULL REFERENCES persons(id),
    movie_id VARCHAR(64) NOT NULL REFERENCES movies(id),
    PRIMARY KEY (person_id, movie_id)
);

CREATE TABLE IF NOT EXISTS wrote (
    person_id VARCHAR(64) NOT NULL REFERENCES persons(id),
    movie_id VARCHAR(64) NOT NULL REFERENCES movies(id),
    PRIMARY KEY (person_id, movie_id)
);

CREATE TABLE IF NOT EXISTS reviewed (
    person_id VARCHAR(64) NOT NULL REFERENCES persons(id),
    movie_id VARCHAR(64) NOT NULL REFERENCES movies(id),
    rating DOUBLE PRECISION NOT NULL,
    summary TEXT,
    PRIMARY KEY (person_id, movie_id)
);

CREATE TABLE IF NOT EXISTS follows (
    follower_id VARCHAR(64) NOT NULL REFERENCES persons(id),
    followed_id VARCHAR(64) NOT NULL REFERENCES persons(id),
    PRIMARY KEY (follower_id, followed_id)
);

-- 2. ISO SQL:2023 Property Graph Definition
CREATE PROPERTY GRAPH IF NOT EXISTS movie_graph
  VERTEX TABLES (
    persons KEY (id) LABEL Person PROPERTIES (id, name, born),
    movies  KEY (id) LABEL Movie  PROPERTIES (id, title, tagline, released)
  )
  EDGE TABLES (
    acted_in
      SOURCE KEY (person_id) REFERENCES persons (id)
      DESTINATION KEY (movie_id) REFERENCES movies (id)
      LABEL ACTED_IN
      PROPERTIES (role),
    directed
      SOURCE KEY (person_id) REFERENCES persons (id)
      DESTINATION KEY (movie_id) REFERENCES movies (id)
      LABEL DIRECTED,
    produced
      SOURCE KEY (person_id) REFERENCES persons (id)
      DESTINATION KEY (movie_id) REFERENCES movies (id)
      LABEL PRODUCED,
    wrote
      SOURCE KEY (person_id) REFERENCES persons (id)
      DESTINATION KEY (movie_id) REFERENCES movies (id)
      LABEL WROTE,
    reviewed
      SOURCE KEY (person_id) REFERENCES persons (id)
      DESTINATION KEY (movie_id) REFERENCES movies (id)
      LABEL REVIEWED
      PROPERTIES (rating, summary),
    follows
      SOURCE KEY (follower_id) REFERENCES persons (id)
      DESTINATION KEY (followed_id) REFERENCES persons (id)
      LABEL FOLLOWS
  );

-- 3. Ingestion of Canonical Records
INSERT INTO movies (id, title, released, tagline) VALUES
  ('TheMatrix', 'The Matrix', 1999, 'Welcome to the Real World'),
  ('TheMatrixReloaded', 'The Matrix Reloaded', 2003, 'Free your mind'),
  ('TheMatrixRevolutions', 'The Matrix Revolutions', 2003, 'Everything that has a beginning has an end'),
  ('TheDevilsAdvocate', 'The Devil''s Advocate', 1997, 'Evil has its winning charm'),
  ('AFewGoodMen', 'A Few Good Men', 1992, 'In the heart of the nation''s capital, in a courthouse of the U.S. government...'),
  ('TopGun', 'Top Gun', 1986, 'I feel the need, the need for speed.'),
  ('JerryMaguire', 'Jerry Maguire', 2000, 'The rest of his life begins now.'),
  ('StandByMe', 'Stand By Me', 1986, 'For some, it''s the last real adventure of a lifetime.'),
  ('CastAway', 'Cast Away', 2000, 'At the edge of the world, his journey begins.'),
  ('Apollo13', 'Apollo 13', 1995, 'Houston, we have a problem.'),
  ('YouveGotMail', 'You''ve Got Mail', 1998, 'At multiplexes and bookstores everywhere.'),
  ('Unforgiven', 'Unforgiven', 1992, 'It''s a hell of a thing, killing a man');

INSERT INTO persons (id, name, born) VALUES
  ('Keanu', 'Keanu Reeves', 1964),
  ('Carrie', 'Carrie-Anne Moss', 1967),
  ('Laurence', 'Laurence Fishburne', 1961),
  ('Hugo', 'Hugo Weaving', 1960),
  ('LillyW', 'Lilly Wachowski', 1967),
  ('LanaW', 'Lana Wachowski', 1965),
  ('JoelS', 'Joel Silver', 1952),
  ('Emil', 'Emil Eifrem', 1978),
  ('Charlize', 'Charlize Theron', 1975),
  ('Al', 'Al Pacino', 1940),
  ('Taylor', 'Taylor Hackford', 1944),
  ('TomC', 'Tom Cruise', 1962),
  ('JackN', 'Jack Nicholson', 1937),
  ('DemiM', 'Demi Moore', 1962),
  ('KevinB', 'Kevin Bacon', 1958),
  ('KieferS', 'Kiefer Sutherland', 1966),
  ('NoahW', 'Noah Wyle', 1971),
  ('CubaG', 'Cuba Gooding Jr.', 1968),
  ('KevinP', 'Kevin Pollak', 1957),
  ('JTW', 'J.T. Walsh', 1943),
  ('JamesM', 'James Marshall', 1967),
  ('ChristopherG', 'Christopher Guest', 1948),
  ('RobR', 'Rob Reiner', 1947),
  ('AaronS', 'Aaron Sorkin', 1961),
  ('KellyM', 'Kelly McGillis', 1957),
  ('ValK', 'Val Kilmer', 1959),
  ('AnthonyE', 'Anthony Edwards', 1962),
  ('TomS', 'Tom Skerritt', 1933),
  ('MegR', 'Meg Ryan', 1961),
  ('TonyS', 'Tony Scott', 1944),
  ('JimC', 'Jim Cash', 1941),
  ('TomH', 'Tom Hanks', 1956),
  ('HelenH', 'Helen Hunt', 1963),
  ('RobertZ', 'Robert Zemeckis', 1951),
  ('EdH', 'Ed Harris', 1950),
  ('BillPax', 'Bill Paxton', 1955),
  ('GaryS', 'Gary Sinise', 1955),
  ('RonH', 'Ron Howard', 1954),
  ('ClintE', 'Clint Eastwood', 1930),
  ('GeneH', 'Gene Hackman', 1930),
  ('MorganF', 'Morgan Freeman', 1937),
  ('PaulB', 'Paul Blythe', NULL),
  ('AngelaS', 'Angela Scope', NULL),
  ('JessicaT', 'Jessica Thompson', NULL),
  ('JamesT', 'James Thompson', NULL);

INSERT INTO acted_in (person_id, movie_id, role) VALUES
  ('Keanu', 'TheMatrix', 'Neo'),
  ('Carrie', 'TheMatrix', 'Trinity'),
  ('Laurence', 'TheMatrix', 'Morpheus'),
  ('Hugo', 'TheMatrix', 'Agent Smith'),
  ('Emil', 'TheMatrix', 'Emil'),
  ('Keanu', 'TheMatrixReloaded', 'Neo'),
  ('Carrie', 'TheMatrixReloaded', 'Trinity'),
  ('Laurence', 'TheMatrixReloaded', 'Morpheus'),
  ('Hugo', 'TheMatrixReloaded', 'Agent Smith'),
  ('Keanu', 'TheMatrixRevolutions', 'Neo'),
  ('Carrie', 'TheMatrixRevolutions', 'Trinity'),
  ('Laurence', 'TheMatrixRevolutions', 'Morpheus'),
  ('Hugo', 'TheMatrixRevolutions', 'Agent Smith'),
  ('Keanu', 'TheDevilsAdvocate', 'Kevin Lomax'),
  ('Charlize', 'TheDevilsAdvocate', 'Mary Ann Lomax'),
  ('Al', 'TheDevilsAdvocate', 'John Milton'),
  ('TomC', 'AFewGoodMen', 'Lt. Daniel Kaffee'),
  ('JackN', 'AFewGoodMen', 'Col. Nathan R. Jessep'),
  ('DemiM', 'AFewGoodMen', 'Lt. Cdr. JoAnne Galloway'),
  ('KevinB', 'AFewGoodMen', 'Capt. Jack Ross'),
  ('TomC', 'TopGun', 'Maverick'),
  ('KellyM', 'TopGun', 'Charlie'),
  ('ValK', 'TopGun', 'Iceman'),
  ('TomH', 'CastAway', 'Chuck Noland'),
  ('HelenH', 'CastAway', 'Kelly Frears'),
  ('TomH', 'Apollo13', 'Jim Lovell'),
  ('KevinB', 'Apollo13', 'Jack Swigert'),
  ('EdH', 'Apollo13', 'Gene Kranz'),
  ('TomH', 'YouveGotMail', 'Joe Fox'),
  ('MegR', 'YouveGotMail', 'Kathleen Kelly'),
  ('ClintE', 'Unforgiven', 'Bill Munny'),
  ('GeneH', 'Unforgiven', 'Little Bill Daggett'),
  ('MorganF', 'Unforgiven', 'Ned Logan');

INSERT INTO directed (person_id, movie_id) VALUES
  ('LillyW', 'TheMatrix'),
  ('LanaW', 'TheMatrix'),
  ('LillyW', 'TheMatrixReloaded'),
  ('LanaW', 'TheMatrixReloaded'),
  ('LillyW', 'TheMatrixRevolutions'),
  ('LanaW', 'TheMatrixRevolutions'),
  ('Taylor', 'TheDevilsAdvocate'),
  ('RobR', 'AFewGoodMen'),
  ('TonyS', 'TopGun'),
  ('RobertZ', 'CastAway'),
  ('RonH', 'Apollo13'),
  ('ClintE', 'Unforgiven');

INSERT INTO produced (person_id, movie_id) VALUES
  ('JoelS', 'TheMatrix'),
  ('JoelS', 'TheMatrixReloaded'),
  ('JoelS', 'TheMatrixRevolutions');

INSERT INTO wrote (person_id, movie_id) VALUES
  ('AaronS', 'AFewGoodMen'),
  ('JimC', 'TopGun');

INSERT INTO reviewed (person_id, movie_id, rating, summary) VALUES
  ('PaulB', 'TheMatrix', 90, 'Moneyball is a great movie!'),
  ('AngelaS', 'Unforgiven', 95, 'Unforgiven is a classic western.'),
  ('JessicaT', 'Apollo13', 88, 'Loved Apollo 13.'),
  ('JamesT', 'YouveGotMail', 85, 'Youve Got Mail was heartwarming.');

INSERT INTO follows (follower_id, followed_id) VALUES
  ('JamesT', 'JessicaT'),
  ('PaulB', 'AngelaS');
