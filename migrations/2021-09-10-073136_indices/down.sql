-- This file should undo anything in `up.sql`-- Your SQL goes here
DROP INDEX idx_sample_run;
DROP INDEX idx_sample_name;
DROP INDEX idx_sample_dna_nr;
DROP INDEX idx_sample_project;
DROP INDEX idx_sample_primer_set;
DROP EXTENSION pg_trgm;
