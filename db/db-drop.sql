DROP IF EXISTS INDEX fastq_filename_idx;
DROP IF EXISTS INDEX fastq_sampleid_idx;


ALTER TABLE IF EXISTS fastq DISABLE TRIGGER ALL;
ALTER TABLE IF EXISTS sample DISABLE TRIGGER ALL;
ALTER TABLE IF EXISTS run DISABLE TRIGGER ALL;
DROP TABLE IF EXISTS fastq CASCADE;
DROP TABLE IF EXISTS run CASCADE;
DROP TABLE IF EXISTS sample CASCADE;
