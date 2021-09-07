-- This file was automatically created by Diesel to setup helper functions
-- and other internal bookkeeping. This file is safe to edit, any future
-- changes will be added to existing projects as new migrations.

DROP FUNCTION IF EXISTS diesel_manage_updated_at(_tbl regclass);
DROP FUNCTION IF EXISTS diesel_set_updated_at();
DROP INDEX IF EXISTS fastq_filename_idx;
DROP INDEX IF EXISTS fastq_sampleid_idx;
ALTER TABLE IF EXISTS fastq DISABLE TRIGGER ALL;
ALTER TABLE IF EXISTS sample DISABLE TRIGGER ALL;
ALTER TABLE IF EXISTS run DISABLE TRIGGER ALL;
DROP TABLE IF EXISTS fastq CASCADE;
DROP TABLE IF EXISTS run CASCADE;
DROP TABLE IF EXISTS sample CASCADE;
