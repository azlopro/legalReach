ALTER TABLE validations 
ADD COLUMN IF NOT EXISTS service VARCHAR(255) DEFAULT 'QuickEmailVerification',
ADD COLUMN IF NOT EXISTS credit_status VARCHAR(255) DEFAULT 'success';

ALTER TABLE import_jobs 
ADD COLUMN IF NOT EXISTS quickemail_validations INTEGER DEFAULT 0,
ADD COLUMN IF NOT EXISTS myemailverifier_validations INTEGER DEFAULT 0,
ADD COLUMN IF NOT EXISTS validation_fallbacks INTEGER DEFAULT 0,
ADD COLUMN IF NOT EXISTS validation_credit_failures INTEGER DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_validations_service ON validations(service);
CREATE INDEX IF NOT EXISTS idx_validations_credit_status ON validations(credit_status);

UPDATE validations SET service = 'QuickEmailVerification' WHERE service = '' OR service IS NULL;
UPDATE validations SET credit_status = 'success' WHERE credit_status = '' OR credit_status IS NULL;