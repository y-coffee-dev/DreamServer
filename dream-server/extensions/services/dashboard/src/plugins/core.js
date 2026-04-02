import { lazy } from 'react'
import {
  LayoutDashboard,
  Settings,
  Puzzle,
  Activity,
} from 'lucide-react'

const Dashboard = lazy(() => import('../pages/Dashboard'))
const SettingsPage = lazy(() => import('../pages/Settings'))
const Extensions = lazy(() => import('../pages/Extensions'))
const GPUMonitor = lazy(() => import('../pages/GPUMonitor'))

export const coreRoutes = [
  {
    id: 'dashboard',
    path: '/',
    label: 'Dashboard',
    icon: LayoutDashboard,
    component: Dashboard,
    getProps: ({ status, loading }) => ({ status, loading }),
    sidebar: true,
    order: 0,
  },
  {
    id: 'gpu-monitor',
    path: '/gpu',
    label: 'GPU Monitor',
    icon: Activity,
    component: GPUMonitor,
    getProps: () => ({}),
    // Route is always registered; sidebar entry only appears on multi-GPU systems
    sidebar: ({ status }) => (status?.gpu?.gpu_count || 1) > 1,
    order: 1,
  },
  {
    id: 'extensions',
    path: '/extensions',
    label: 'Extensions',
    icon: Puzzle,
    component: Extensions,
    getProps: () => ({}),
    sidebar: true,
  },
  {
    id: 'settings',
    path: '/settings',
    label: 'Settings',
    icon: Settings,
    component: SettingsPage,
    getProps: () => ({}),
    sidebar: true,
    order: 99,
  },
]

export const coreExternalLinks = []
