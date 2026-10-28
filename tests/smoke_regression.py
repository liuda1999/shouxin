#!/usr/bin/env python3
"""
Smoke Regression Test after Security Fix
========================================
Quick smoke test to verify all functionality works after security fixes
"""
import requests, time, os, sys
from datetime import datetime, timezone

BASE = 'http://127.0.0.1:3000'
TOKEN = 'shouxin-v050-test-security-fixed'
TEST_USER = 'smoke_test_user'
PASS = 0
FAIL = 0

def t(name, ok, detail=''):
    global PASS, FAIL
    if ok:
        PASS += 1
        print(f'  [PASS] {name}' + (f' - {detail}' if detail else ''))
    else:
        FAIL += 1
        print(f'  [FAIL] {name} - {detail}')

print('=== SMOKE TEST (Security Fix Regression) ===')
print()

# Health
try:
    r = requests.get(f'{BASE}/health', timeout=5)
    t('GET /health', r.status_code == 200, str(r.status_code))
except Exception as e:
    t('GET /health', False, str(e))

# Files
try:
    r = requests.post(
        f'{BASE}/api/files/upload',
        params={'filename': 'smoke.txt', 'scope': 'User', 'user_id': TEST_USER},
        data=b'hello world',
        headers={'Content-Type': 'application/octet-stream'}
    )
    file_id = r.json().get('data', {}).get('file_id', '')
    t('POST /api/files/upload', r.status_code == 200, str(r.status_code))

    r = requests.get(f'{BASE}/api/files')
    t('GET /api/files', r.status_code == 200, str(r.status_code))

    if file_id:
        r = requests.get(f'{BASE}/api/files/{file_id}')
        t('GET /api/files/:id', r.status_code == 200, str(r.status_code))
except Exception as e:
    t('Files API', False, str(e))

# Todos
try:
    for i in range(3):
        r = requests.post(
            f'{BASE}/api/todos',
            json={
                'title': f'Smoke #{i}',
                'description': 'test',
                'scope': 'User',
                'user_id': TEST_USER,
                'deadline': datetime.now(timezone.utc).replace(hour=23).isoformat(),
                'importance': 'medium'
            }
        )
    t(f'POST /api/todos (3 todos)', r.status_code == 200, str(r.status_code))

    r = requests.get(f'{BASE}/api/todos', params={'scope': 'User', 'user_id': TEST_USER, 'limit': 10})
    data = r.json().get('data', [])
    t('GET /api/todos', r.status_code == 200, f'count={len(data)}')
except Exception as e:
    t('Todos API', False, str(e))

# Internal API with correct token
try:
    r = requests.post(
        f'{BASE}/internal/agent/register',
        json={'agent_id': 'smoke_agent', 'name': 'Smoke'},
        headers={'Authorization': f'Bearer {TOKEN}'}
    )
    t('Internal /agent/register', r.status_code in [201, 409], str(r.status_code))

    r = requests.post(
        f'{BASE}/internal/event/add',
        json={
            'description': 'smoke',
            'related_contacts': ['c1'],
            'source_agent_id': 'smoke_agent',
            'owner_type': 'Agent',
            'owner_id': 'smoke_agent'
        },
        headers={'Authorization': f'Bearer {TOKEN}'}
    )
    t('Internal /event/add', r.status_code == 201, str(r.status_code))
except Exception as e:
    t('Internal API', False, str(e))

# Internal API with wrong token (should fail)
try:
    r = requests.post(
        f'{BASE}/internal/event/add',
        json={'description': 'bad'},
        headers={'Authorization': 'Bearer wrong'}
    )
    t('Internal with wrong token -> 401', r.status_code == 401, str(r.status_code))
except Exception as e:
    t('Internal wrong token test', False, str(e))

# Admin API with correct token
try:
    r = requests.get(
        f'{BASE}/admin/llm/status',
        headers={'X-Internal-Token': TOKEN}
    )
    t('Admin /llm/status', r.status_code == 200, str(r.status_code))
except Exception as e:
    t('Admin API', False, str(e))

# Admin API with wrong token (should fail)
try:
    r = requests.get(
        f'{BASE}/admin/llm/status',
        headers={'X-Internal-Token': 'wrong'}
    )
    t('Admin with wrong token -> 401', r.status_code == 401, str(r.status_code))
except Exception as e:
    t('Admin wrong token test', False, str(e))

# Settings
try:
    r = requests.get(f'{BASE}/api/settings')
    t('GET /api/settings', r.status_code == 200, str(r.status_code))
except Exception as e:
    t('GET /api/settings', False, str(e))

# Contacts
try:
    r = requests.get(f'{BASE}/api/contacts')
    t('GET /api/contacts', r.status_code == 200, str(r.status_code))
except Exception as e:
    t('GET /api/contacts', False, str(e))

# Knowledge
try:
    r = requests.get(f'{BASE}/api/knowledge')
    t('GET /api/knowledge', r.status_code == 200, str(r.status_code))
except Exception as e:
    t('GET /api/knowledge', False, str(e))

# Events
try:
    r = requests.get(f'{BASE}/api/events', params={'limit': 10})
    t('GET /api/events', r.status_code == 200, str(r.status_code))
except Exception as e:
    t('GET /api/events', False, str(e))

# Reminders
try:
    r = requests.get(f'{BASE}/api/reminders/upcoming', params={'hours': 48, 'limit': 10})
    t('GET /api/reminders/upcoming', r.status_code == 200, str(r.status_code))
except Exception as e:
    t('GET /api/reminders', False, str(e))

# Summary
print()
print(f'=== RESULT: {PASS}/{PASS+FAIL} passed ({100*PASS//max(PASS+FAIL,1)}%) ===')

if FAIL > 0:
    sys.exit(1)
