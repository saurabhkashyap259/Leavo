import { useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { createRoute, createRouter, redirect, useNavigate, useRouter } from '@tanstack/react-router'
import { ApiError } from './api/client'
import { orgQuery } from './api/admin'
import { meQuery } from './api/me'
import { setupStatusQuery } from './api/setup'
import { Layout } from './components/Layout'
import { rootRoute } from './routes/__root'
import { markChecklistSeen } from './routes/admin/checklist'
import { ApproversPage } from './routes/admin/approvers'
import { AuditPage } from './routes/admin/audit'
import { CarryForwardPage } from './routes/admin/carry-forward'
import { EmployeePage } from './routes/admin/employee'
import { EmployeesPage } from './routes/admin/employees'
import { ImportPage } from './routes/admin/import'
import { InstancePage } from './routes/admin/instance'
import { ReportsPage } from './routes/admin/reports'
import { AdminHolidaysPage } from './routes/admin/holidays'
import { AdminHome } from './routes/admin/index'
import { HolidaysPage } from './routes/holidays'
import { OrgPage } from './routes/admin/org'
import { PolicyPage } from './routes/admin/policy'
import { ApprovalsPage, type LinkNotice, type Prefill } from './routes/approvals'
import { DashboardPage } from './routes/dashboard'
import { RegisterPage } from './routes/register'
import { SetupWizard } from './routes/setup/index'
import { ApplyLeavePage } from './routes/leave/apply'
import { CompOffsPage } from './routes/comp-offs'
import { HistoryPage } from './routes/leave/history'
import { LoginPage } from './routes/login'
import { ProfilePage } from './routes/profile'
import { SignupPage } from './routes/signup'
import { TeamPage } from './routes/team'

// Signed-in area: every child route requires a session (redirects to /login on 401).
const appRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: 'app',
  beforeLoad: async ({ context, location }) => {
    await requireSetupDone(context.queryClient)
    try {
      const me = await context.queryClient.ensureQueryData(meQuery)
      return { me }
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        throw redirect({ to: '/login', search: { next: location.href } })
      }
      throw e
    }
  },
  component: function AppLayout() {
    const { me } = appRoute.useRouteContext()
    return <Layout me={me} />
  },
})

/** A fresh install sends every page to the setup wizard (FR-007). */
async function requireSetupDone(queryClient: QueryClient) {
  const status = await queryClient.ensureQueryData(setupStatusQuery)
  if (status.setup_required) throw redirect({ to: '/setup' })
}

const adminRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/admin',
  beforeLoad: ({ context }) => {
    if (context.me.role !== 'admin') throw redirect({ to: '/dashboard' })
  },
})

const adminIndexRoute = createRoute({
  getParentRoute: () => adminRoute,
  path: '/',
  component: AdminHome,
})

const adminOrgRoute = createRoute({
  getParentRoute: () => adminRoute,
  path: '/org',
  component: function AdminOrg() {
    markChecklistSeen('domains')
    return <OrgPage />
  },
})

const adminPolicyRoute = createRoute({
  getParentRoute: () => adminRoute,
  path: '/policy',
  component: function AdminPolicy() {
    markChecklistSeen('policy')
    return <PolicyPage />
  },
})

const adminHolidaysRoute = createRoute({
  getParentRoute: () => adminRoute,
  path: '/holidays',
  component: function AdminHolidays() {
    markChecklistSeen('holidays')
    const { me } = appRoute.useRouteContext()
    return <AdminHolidaysPage leaveYearStartMonth={me.org.leave_year_start_month} />
  },
})

const adminEmployeesRoute = createRoute({
  getParentRoute: () => adminRoute,
  path: '/employees',
  component: function AdminEmployees() {
    markChecklistSeen('invite')
    return <EmployeesPage />
  },
})

const adminEmployeeRoute = createRoute({
  getParentRoute: () => adminRoute,
  path: '/employees/$id',
  component: function AdminEmployee() {
    const { id } = adminEmployeeRoute.useParams()
    const { me } = appRoute.useRouteContext()
    return <EmployeePage key={id} id={id} leaveYearStartMonth={me.org.leave_year_start_month} />
  },
})

const adminApproversRoute = createRoute({
  getParentRoute: () => adminRoute,
  path: '/approvers',
  component: function AdminApprovers() {
    markChecklistSeen('approvers')
    return <ApproversPage />
  },
})

const adminReportsRoute = createRoute({
  getParentRoute: () => adminRoute,
  path: '/reports',
  component: function AdminReports() {
    const { me } = appRoute.useRouteContext()
    return <ReportsPage leaveYearStartMonth={me.org.leave_year_start_month} />
  },
})

const adminImportRoute = createRoute({
  getParentRoute: () => adminRoute,
  path: '/import',
  component: function AdminImport() {
    const { me } = appRoute.useRouteContext()
    return <ImportPage leaveYearStartMonth={me.org.leave_year_start_month} />
  },
})

const adminInstanceRoute = createRoute({
  getParentRoute: () => adminRoute,
  path: '/instance',
  component: function AdminInstance() {
    // Signing everyone out ends this session too: back to the sign-in page.
    return <InstancePage onSignedOut={() => window.location.assign('/login')} />
  },
})

const adminAuditRoute = createRoute({
  getParentRoute: () => adminRoute,
  path: '/audit',
  component: AuditPage,
})

const adminCarryForwardRoute = createRoute({
  getParentRoute: () => adminRoute,
  path: '/carry-forward',
  component: function AdminCarryForward() {
    const { me } = appRoute.useRouteContext()
    const org = useQuery(orgQuery)
    return (
      <CarryForwardPage
        leaveYearStartMonth={me.org.leave_year_start_month}
        carryForwardCap={org.data?.carry_forward_cap ?? 0}
      />
    )
  },
})

const holidaysRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/holidays',
  component: function Holidays() {
    const { me } = appRoute.useRouteContext()
    return <HolidaysPage leaveYearStartMonth={me.org.leave_year_start_month} />
  },
})

const indexRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/',
  beforeLoad: () => {
    throw redirect({ to: '/dashboard' })
  },
})

const dashboardRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/dashboard',
  component: DashboardPage,
})

const applyLeaveRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/leave/apply',
  component: ApplyLeavePage,
})

const historyRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/leave/history',
  component: HistoryPage,
})

const compOffsRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/comp-offs',
  component: CompOffsPage,
})

const teamRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/team',
  component: TeamPage,
})

type ApprovalsSearch = {
  request?: string
  decision?: 'approve' | 'reject'
  t?: string
  link?: LinkNotice
}

/** `/approvals?request=&decision=&t=` comes from an email link (FR-024); `link=` explains a dead one. */
const validateApprovals = (s: Record<string, unknown>): ApprovalsSearch => ({
  ...(typeof s.request === 'string' ? { request: s.request } : {}),
  ...(s.decision === 'approve' || s.decision === 'reject' ? { decision: s.decision } : {}),
  ...(typeof s.t === 'string' ? { t: s.t } : {}),
  ...(s.link === 'used' || s.link === 'expired' || s.link === 'invalid' ? { link: s.link } : {}),
})

const approvalsRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/approvals',
  validateSearch: validateApprovals,
  beforeLoad: ({ context }) => {
    if (context.me.role === 'employee') throw redirect({ to: '/dashboard' })
  },
  component: function Approvals() {
    const s = approvalsRoute.useSearch()
    const prefill: Prefill | undefined =
      s.request && s.decision ? { request: s.request, decision: s.decision, token: s.t } : undefined
    return <ApprovalsPage key={s.request ?? 'inbox'} prefill={prefill} link={s.link} />
  },
})

const profileRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/profile',
  component: function Profile() {
    const queryClient = useQueryClient()
    const navigate = useNavigate()
    return (
      <ProfilePage
        onSignedOut={() => {
          queryClient.clear()
          void navigate({ to: '/login', search: {} })
        }}
      />
    )
  },
})

type NextSearch = { next?: string }

/** Only same-site paths are accepted as a post-sign-in destination (no open redirect). */
const validateNext = (s: Record<string, unknown>): NextSearch =>
  typeof s.next === 'string' && s.next.startsWith('/') && !s.next.startsWith('//')
    ? { next: s.next }
    : {}

/** After sign-in: refresh the session-dependent data, then go where the user was headed. */
function useAfterSignIn(next: string | undefined) {
  const router = useRouter()
  return async () => {
    router.options.context.queryClient.clear()
    await router.navigate({ to: next ?? '/dashboard' })
  }
}

const setupRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/setup',
  beforeLoad: async ({ context }) => {
    const status = await context.queryClient.fetchQuery({ ...setupStatusQuery, staleTime: 0 })
    if (!status.setup_required) throw redirect({ to: '/login', search: {} })
    return { status }
  },
  component: function Setup() {
    const { status } = setupRoute.useRouteContext()
    const router = useRouter()
    return (
      <SetupWizard
        packs={status.holiday_packs}
        onDone={async () => {
          router.options.context.queryClient.clear()
          await router.navigate({ to: '/admin' })
        }}
      />
    )
  },
})

const registerRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/register',
  beforeLoad: async ({ context }) => {
    const status = await context.queryClient.ensureQueryData(setupStatusQuery)
    if (status.mode !== 'multi') throw redirect({ to: '/login', search: {} })
  },
  component: function Register() {
    const router = useRouter()
    return (
      <RegisterPage
        onDone={async () => {
          router.options.context.queryClient.clear()
          await router.navigate({ to: '/admin' })
        }}
      />
    )
  },
})

const loginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/login',
  validateSearch: validateNext,
  beforeLoad: ({ context }) => requireSetupDone(context.queryClient),
  component: function Login() {
    const { next } = loginRoute.useSearch()
    return <LoginPage onSignedIn={useAfterSignIn(next)} />
  },
})

const signupRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/signup',
  validateSearch: validateNext,
  beforeLoad: ({ context }) => requireSetupDone(context.queryClient),
  component: function Signup() {
    const { next } = signupRoute.useSearch()
    return <SignupPage onSignedIn={useAfterSignIn(next)} />
  },
})

const routeTree = rootRoute.addChildren([
  appRoute.addChildren([
    indexRoute,
    dashboardRoute,
    applyLeaveRoute,
    historyRoute,
    teamRoute,
    holidaysRoute,
    compOffsRoute,
    approvalsRoute,
    profileRoute,
    adminRoute.addChildren([
      adminIndexRoute,
      adminOrgRoute,
      adminPolicyRoute,
      adminHolidaysRoute,
      adminCarryForwardRoute,
      adminEmployeesRoute,
      adminEmployeeRoute,
      adminApproversRoute,
      adminReportsRoute,
      adminAuditRoute,
      adminImportRoute,
      adminInstanceRoute,
    ]),
  ]),
  setupRoute,
  registerRoute,
  loginRoute,
  signupRoute,
])

export function makeRouter(queryClient: import('@tanstack/react-query').QueryClient) {
  return createRouter({ routeTree, context: { queryClient }, defaultPreload: 'intent' })
}

declare module '@tanstack/react-router' {
  interface Register {
    router: ReturnType<typeof makeRouter>
  }
}
