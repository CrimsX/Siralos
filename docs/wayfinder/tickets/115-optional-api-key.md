---
title: "The Optional API Key"
label: "wayfinder:ticket"
status: closed
date: "2026-08-31"
supersedes: []
---

# The Optional API Key

Public endpoints need no key. The api key field in the provider add-form
becomes optional — empty = no credential — matching the profile schema,
which already treats `credential` as absent-able
(`profile_config` parses it into `Option<String>`).

Authorized by
[the Optional API Key Entry Review](../decisions/135-optional-api-key-entry-review.md).
