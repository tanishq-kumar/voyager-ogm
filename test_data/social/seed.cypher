// ==========================================
// Voyager OGM Test Dataset: Social Network
// Dialect: openCypher (Neo4j / Memgraph)
// ==========================================

// Create Persons
CREATE (p1:Person {id: 'usr_001', firstName: 'Ada', lastName: 'Lovelace', gender: 'F', birthday: '1815-12-10', email: 'ada@example.com', locationIp: '192.168.1.10'})
CREATE (p2:Person {id: 'usr_002', firstName: 'Alan', lastName: 'Turing', gender: 'M', birthday: '1912-06-23', email: 'alan@example.com', locationIp: '192.168.1.11'})
CREATE (p3:Person {id: 'usr_003', firstName: 'Grace', lastName: 'Hopper', gender: 'F', birthday: '1906-12-09', email: 'grace@example.com', locationIp: '192.168.1.12'})
CREATE (p4:Person {id: 'usr_004', firstName: 'Claude', lastName: 'Shannon', gender: 'M', birthday: '1916-04-30', email: 'claude@example.com', locationIp: '192.168.1.13'})
CREATE (p5:Person {id: 'usr_005', firstName: 'John', lastName: 'von Neumann', gender: 'M', birthday: '1903-12-28', email: 'john@example.com', locationIp: '192.168.1.14'})
CREATE (p6:Person {id: 'usr_006', firstName: 'Margaret', lastName: 'Hamilton', gender: 'F', birthday: '1936-08-17', email: 'margaret@example.com', locationIp: '192.168.1.15'});

// Create Tags
CREATE (t1:Tag {id: 'tag_ai', name: 'Artificial Intelligence', url: 'https://topics/ai'})
CREATE (t2:Tag {id: 'tag_comp', name: 'Compiler Engineering', url: 'https://topics/compilers'})
CREATE (t3:Tag {id: 'tag_graph', name: 'Graph Databases', url: 'https://topics/graphs'})
CREATE (t4:Tag {id: 'tag_info', name: 'Information Theory', url: 'https://topics/info_theory'});

// Create Posts
CREATE (post1:Post {id: 'pst_101', content: 'Computing machinery and intelligence can transform science.', length: 60, browserUsed: 'Chrome', creationDate: 1700000100})
CREATE (post2:Post {id: 'pst_102', content: 'A symbolic analysis of relay and switching circuits.', length: 53, browserUsed: 'Firefox', creationDate: 1700000200})
CREATE (post3:Post {id: 'pst_103', content: 'Compilers bridge human logic and silicon instructions seamlessly.', length: 67, browserUsed: 'Safari', creationDate: 1700000300})
CREATE (post4:Post {id: 'pst_104', content: 'Software engineering got us safely to the moon.', length: 48, browserUsed: 'Edge', creationDate: 1700000400});

// Relationships: CREATOR_OF
CREATE (p2)-[:CREATOR_OF {creationDate: 1700000100}]->(post1)
CREATE (p4)-[:CREATOR_OF {creationDate: 1700000200}]->(post2)
CREATE (p3)-[:CREATOR_OF {creationDate: 1700000300}]->(post3)
CREATE (p6)-[:CREATOR_OF {creationDate: 1700000400}]->(post4);

// Relationships: HAS_TAG
CREATE (post1)-[:HAS_TAG]->(t1)
CREATE (post1)-[:HAS_TAG]->(t3)
CREATE (post2)-[:HAS_TAG]->(t4)
CREATE (post3)-[:HAS_TAG]->(t2)
CREATE (post4)-[:HAS_TAG]->(t2);

// Relationships: LIKES
CREATE (p1)-[:LIKES {creationDate: 1700000500}]->(post1)
CREATE (p3)-[:LIKES {creationDate: 1700000550}]->(post1)
CREATE (p4)-[:LIKES {creationDate: 1700000600}]->(post1)
CREATE (p2)-[:LIKES {creationDate: 1700000650}]->(post2)
CREATE (p1)-[:LIKES {creationDate: 1700000700}]->(post3)
CREATE (p6)-[:LIKES {creationDate: 1700000750}]->(post3)
CREATE (p1)-[:LIKES {creationDate: 1700000800}]->(post4)
CREATE (p3)-[:LIKES {creationDate: 1700000850}]->(post4);

// Relationships: KNOWS (Friendship network with multi-hop paths)
CREATE (p1)-[:KNOWS {creationDate: 1680000000, closenessScore: 0.95}]->(p2)
CREATE (p2)-[:KNOWS {creationDate: 1680000000, closenessScore: 0.95}]->(p1)

CREATE (p2)-[:KNOWS {creationDate: 1680000100, closenessScore: 0.88}]->(p3)
CREATE (p3)-[:KNOWS {creationDate: 1680000100, closenessScore: 0.88}]->(p2)

CREATE (p2)-[:KNOWS {creationDate: 1680000200, closenessScore: 0.92}]->(p5)
CREATE (p5)-[:KNOWS {creationDate: 1680000200, closenessScore: 0.92}]->(p2)

CREATE (p3)-[:KNOWS {creationDate: 1680000300, closenessScore: 0.85}]->(p6)
CREATE (p6)-[:KNOWS {creationDate: 1680000300, closenessScore: 0.85}]->(p3)

CREATE (p4)-[:KNOWS {creationDate: 1680000400, closenessScore: 0.90}]->(p5)
CREATE (p5)-[:KNOWS {creationDate: 1680000400, closenessScore: 0.90}]->(p4)

CREATE (p1)-[:KNOWS {creationDate: 1680000500, closenessScore: 0.78}]->(p6)
CREATE (p6)-[:KNOWS {creationDate: 1680000500, closenessScore: 0.78}]->(p1);
