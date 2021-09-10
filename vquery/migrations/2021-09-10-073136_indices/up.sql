-- Your SQL goes here
CREATE EXTENSION pg_trgm;
CREATE INDEX idx_sample_run ON sample USING gin (run gin_trgm_ops);
CREATE INDEX idx_sample_name ON sample USING gin (name gin_trgm_ops);
CREATE INDEX idx_sample_dna_nr ON sample USING gin (dna_nr gin_trgm_ops);
CREATE INDEX idx_sample_project ON sample USING gin (project gin_trgm_ops);
CREATE INDEX idx_sample_primer_set ON sample USING gin (primer_set gin_trgm_ops);

