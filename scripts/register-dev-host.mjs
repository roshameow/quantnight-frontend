#!/usr/bin/env node
// Explicit operator setup, never a beforeDev hook or automatic installation.
import { parseArgs } from 'node:util';
import { enrollmentPlan, writeEnrollment } from './dev-registration.mjs';
const { values } = parseArgs({ options: {
  mode:{type:'string',default:'dev'}, 'frontend-root':{type:'string'}, 'backend-root':{type:'string'},
  executable:{type:'string'}, binding:{type:'string'}, target:{type:'string'}, write:{type:'boolean',default:false},
} });
try {
  const plan = enrollmentPlan({ mode:values.mode,frontendRoot:values['frontend-root'],backendRoot:values['backend-root'],
    executable:values.executable,bindingPath:values.binding,target:values.target });
  if (values.write) writeEnrollment(plan);
  console.log(JSON.stringify({ written:values.write, files:plan },null,2));
} catch (e) { console.error(`Explicit enrollment failed: ${e.message}`); process.exitCode=1; }
