-- Sign-in is by email only (a link or a code in one email), so the passkey table Better Auth's
-- passkey plugin used is gone, with its indexes. The email code needs no table of its own: it is a
-- row in "verification", like the link.
drop table if exists "passkey";
