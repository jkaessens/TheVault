-- This file should undo anything in `up.sql`
ALTER TABLE sample ALTER COLUMN dna_nr SET NOT NULL;
