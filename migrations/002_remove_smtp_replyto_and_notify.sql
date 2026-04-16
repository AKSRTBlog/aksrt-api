-- Remove reply_to_email and admin_notify_email from smtp_configs
-- Notification emails are now sent to the submitter + all active admin emails
ALTER TABLE smtp_configs DROP COLUMN IF EXISTS reply_to_email;
ALTER TABLE smtp_configs DROP COLUMN IF EXISTS admin_notify_email;
