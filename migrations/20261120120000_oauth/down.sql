DELETE FROM user_api_keys
    WHERE uuid IN (SELECT api_key_uuid FROM dev_caloptreyx_calmcp_oauth_grants);
DROP TABLE IF EXISTS dev_caloptreyx_calmcp_oauth_tokens;
DROP TABLE IF EXISTS dev_caloptreyx_calmcp_oauth_codes;
DROP TABLE IF EXISTS dev_caloptreyx_calmcp_oauth_grants;
DROP TABLE IF EXISTS dev_caloptreyx_calmcp_oauth_clients;
