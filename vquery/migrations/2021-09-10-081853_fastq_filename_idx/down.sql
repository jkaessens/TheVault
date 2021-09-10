-- This file should undo anything in `up.sql`-- Your SQL goes here
DROP INDEX idx_fastq_filename_gin;
CREATE INDEX fastq_filename_idx ON fastq USING btree (filename);
