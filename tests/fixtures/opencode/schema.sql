-- The subset of OpenCode's schema (v2.4) that Runeway reads.
CREATE TABLE session_v2 (
  id text PRIMARY KEY,
  parent_id text,
  directory text NOT NULL,
  time_updated integer NOT NULL,
  time_busy integer,
  time_idle integer,
  time_archived integer,
  time_suspended integer,
  tokens_input integer DEFAULT 0 NOT NULL,
  tokens_output integer DEFAULT 0 NOT NULL,
  tokens_reasoning integer DEFAULT 0 NOT NULL
);
CREATE TABLE session_message (
  id text PRIMARY KEY,
  session_id text NOT NULL,
  type text NOT NULL,
  seq integer NOT NULL,
  data text NOT NULL
);
