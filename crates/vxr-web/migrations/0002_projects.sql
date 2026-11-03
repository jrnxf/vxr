-- The cloud projects: one row per project in an account, its bytes in R2 under
-- <user_id>/<id>/<version>.vox and its thumbnail under <user_id>/<id>/thumb.png. `version` is a
-- fresh random id on every save, so a save says which version it replaces (If-Match) and a save
-- from a stale copy is kept beside the newer one instead of over it. `hash` (sha-256 of the
-- bytes) makes a repeated save of the same bytes a no-op instead of a conflict.
create table project (
  user_id text not null references "user" ("id") on delete cascade,
  id text not null,
  name text not null,
  version text not null,
  hash text not null,
  size integer not null,
  thumb integer not null default 0,
  device text not null default '',
  created_at integer not null,
  updated_at integer not null,
  primary key (user_id, id)
);

create index project_user_updated on project (user_id, updated_at);

-- Every sign-in link asked for, by address and by IP, to limit them per hour.
create table mail_log (
  email text not null,
  ip text not null,
  at integer not null
);

create index mail_log_email on mail_log (email, at);

create index mail_log_ip on mail_log (ip, at);
