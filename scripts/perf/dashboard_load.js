// T225 — dashboard load check (SC-003, SC-013), for k6 (https://k6.io, latest).
//
// Every iteration is one dashboard visit by a random signed-in employee: the five API calls the
// dashboard page makes, sent in parallel like a browser does. About one visit in ten is an admin
// opening the admin dashboard instead. The check fails unless 95% of visits finish in under
// 1.5 s and fewer than 1% of requests fail.
//
// Run it through scripts/perf/run.sh, which seeds the organization and creates the sessions this
// script signs in with. Settings (environment variables):
//   BASE_URL    server under test                default http://127.0.0.1:18090
//   DOMAIN      the seeded organization's domain default perf.leavo.test
//   EMPLOYEES   generated employees (person1…N)  default 1000
//   VUS         people using it at once          default 50
//   DURATION    how long to keep the load on     default 2m
import { check, group, sleep } from 'k6'
import http from 'k6/http'

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:18090'
const DOMAIN = __ENV.DOMAIN || 'perf.leavo.test'
const EMPLOYEES = Number(__ENV.EMPLOYEES || 1000)
const API = `${BASE_URL}/api/v1`

export const options = {
  scenarios: {
    dashboard: {
      executor: 'constant-vus',
      vus: Number(__ENV.VUS || 50),
      duration: __ENV.DURATION || '2m',
    },
  },
  thresholds: {
    // SC-003: the dashboard loads in under 1.5 s for 95% of visits.
    'group_duration{group:::employee dashboard}': ['p(95)<1500'],
    'group_duration{group:::admin dashboard}': ['p(95)<1500'],
    http_req_failed: ['rate<0.01'],
  },
}

/** run.sh gives each seeded user the session token `perf-<email>`. */
function as(email) {
  return { headers: { Cookie: `leavo_sid=perf-${email}` }, tags: {} }
}

function ok(responses) {
  check(responses, {
    'every call succeeded': (rs) => rs.every((r) => r.status === 200),
  })
}

export default function () {
  const year = new Date().getFullYear()
  if (Math.random() < 0.1) {
    group('admin dashboard', () => {
      const p = as(`admin@${DOMAIN}`)
      ok(
        http.batch([
          ['GET', `${API}/me`, null, p],
          ['GET', `${API}/admin/dashboard`, null, p],
        ]),
      )
    })
  } else {
    const n = 1 + Math.floor(Math.random() * EMPLOYEES)
    group('employee dashboard', () => {
      const p = as(`person${n}@${DOMAIN}`)
      ok(
        http.batch([
          ['GET', `${API}/me`, null, p],
          ['GET', `${API}/me/balances`, null, p],
          ['GET', `${API}/away?range=today&scope=org`, null, p],
          ['GET', `${API}/me/leaves?limit=50`, null, p],
          ['GET', `${API}/holidays?year=${year}`, null, p],
        ]),
      )
    })
  }
  // Think time: a person reads the page before the next visit.
  sleep(1 + Math.random() * 2)
}
