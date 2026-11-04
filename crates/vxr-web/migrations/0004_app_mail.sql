-- Sign-in emails the desktop app asked for (no Turnstile there: see worker/index.ts), counted
-- apart so they have tighter limits of their own, per IP and for all of them together.
alter table mail_log add column app integer not null default 0;

create index mail_log_app on mail_log (app, at);
