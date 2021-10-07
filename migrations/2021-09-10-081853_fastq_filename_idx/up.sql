-- Your SQL goes here
DROP INDEX fastq_filename_idx;
CREATE INDEX idx_fastq_filename_gin ON fastq USING gin (filename gin_trgm_ops);

