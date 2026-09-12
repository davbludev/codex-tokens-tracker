import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { pathToFileURL } from "node:url";
import { installDashboardFixture } from "./dashboard-fixture.mjs";

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ? pathToFileURL(process.env.PLAYWRIGHT_MODULE).href : "playwright");
const server=spawn(process.execPath,["node_modules/vite/bin/vite.js","--host","127.0.0.1","--port","1425","--strictPort"],{windowsHide:true,stdio:"pipe"});
let browser;
try {
  await new Promise((resolve,reject)=>{ const timer=setTimeout(()=>reject(new Error("Vite did not start")),15000); server.stdout.on("data",value=>{if(value.toString().includes("1425")){clearTimeout(timer);resolve();}});server.on("exit",code=>{clearTimeout(timer);reject(new Error(`Vite exited ${code}`));}); });
  browser=await chromium.launch({channel:"msedge",headless:true});
  const page=await browser.newPage({viewport:{width:1440,height:1000},timezoneId:"America/Edmonton"});
  const failures=[];page.on("pageerror",error=>failures.push(error.message));
  await page.addInitScript(installDashboardFixture);
  await page.addInitScript(()=>{
    const native=window.__TAURI_INTERNALS__.invoke;
    const state=window.dashboardTest;
    const t=seconds=>({seconds,nanos:0});
    const n=value=>({knownTokens:String(value),complete:true});
    const cost=value=>({knownSubtotal:String(value),complete:true});
    const billed=(tokens,amount)=>({tokens:n(tokens),estimatedCost:cost(amount)});
    const split={input:billed(20,40000000),cachedInput:billed(80,16000000),cacheWrites:billed(0,0),output:billed(20,200000000)};
    const start=state.response.localUsage.start.seconds,end=state.response.localUsage.end.seconds;
    const original=Array.from({length:101},(_,index)=>({time:t(Math.round(start+(end-start)*index/100)),segmentId:"segment",weeklyUsedPercent:String(20+index/2),cumulativeEstimatedCost:cost(BigInt(index)*100000000000n),effectiveUsdPerPercent:index?String(5+(index%7)):null,unavailableReason:index?null:"insufficientObservations",connectFromPrevious:index>0}));
    const calls=Array.from({length:55},(_,index)=>({id:String(index+1),time:t(Math.round(start+(index+.5)*(end-start)/55)),threadId:index%2?"task-b":"task-a",turnId:"one-turn-many-calls",responseId:`response-${index+1}`,model:index%2?"model-b":"model-a",effort:"high",tokens:{totalTokens:n(120),inputTokens:n(100),cachedInputTokens:n(80),cacheWriteTokens:n(0),outputTokens:n(20),reasoningTokens:n(5)},categories:split,estimatedCost:cost(256000000),priceVersionId:"1",price:{input:"2",cachedInput:"0.2",output:"10",cacheWrite:"3",reasoning:null,reasoningPolicy:"included",cacheWritePolicy:"additional"},categoryReason:null}));
    const test=window.callsTest={requests:[],completed:[],activity:[],text:[],original,calls,held:[],hold:false};
    const seconds=time=>time.seconds+time.nanos/1e9;
    const selected=query=>calls.filter(call=>seconds(call.time)>seconds(query.start)&&seconds(call.time)<=seconds(query.end)&&(!query.model||call.model===query.model)&&(!query.thread||call.threadId===query.thread));
    const totals=items=>({categories:Object.fromEntries(Object.entries(split).map(([key,value])=>[key,{tokens:n(BigInt(value.tokens.knownTokens)*BigInt(items.length)),estimatedCost:cost(BigInt(value.estimatedCost.knownSubtotal)*BigInt(items.length))}])),estimatedCost:cost(BigInt(items.length)*256000000n)});
    window.__TAURI_INTERNALS__.invoke=async(command,args)=>{
      if(command==="usage_dashboard"){
        const result=await native(command,args); const query=args.query;
        const resolvedEnd=query.range==="custom"?query.end:t(end);
        const resolvedStart=query.range==="custom"?query.start:query.range==="trailing"?t(resolvedEnd.seconds-query.durationSeconds):t(start);
        result.chart.start=resolvedStart;result.chart.end=resolvedEnd;result.chart.points=original.filter(point=>seconds(point.time)>=seconds(resolvedStart)&&seconds(point.time)<=seconds(resolvedEnd));result.chart.boundaries=[];
        result.localUsage.start=resolvedStart;result.localUsage.end=resolvedEnd; result.turnActivity.start=resolvedStart;result.turnActivity.end=resolvedEnd;
        result.availableStart=t(start);
        result.availableEnd=calls.at(-1).time;
        const points=result.chart.points;const last=points.at(-1);
        result.rangeQuota={latest:last?{time:last.time,usedPercent:last.weeklyUsedPercent,remainingPercent:String(100-Number(last.weeklyUsedPercent)),resetsAt:null}:null,segments:[{start:points[0]?.time??null,end:last?.time??null,consumedPercentagePoints:"2",estimatedCost:cost(512000000),effectiveUsdPerPercent:"0.000256",estimatedFullWeekUsd:"0.0256",unavailableReason:points.length>1?null:"insufficientObservations"}],totalSegments:1,recent:{...result.weekly.recent},unmatchedCost:cost(0)};
        return result;
      }
      if(command==="usage_calls"){
        if(Object.keys(args.query).some(key=>!["start","end","model","thread","after","limit"].includes(key))) throw "invalidQuery";
        test.requests.push(structuredClone(args.query));
        const values=selected(args.query),offset=Number(args.query.after??0),items=values.slice(offset,offset+50);
        const response={start:args.query.start,end:args.query.end,items,totalItems:values.length,summary:totals(values),nextCursor:offset+50<values.length?String(offset+50):null};
        if(test.hold) await new Promise(resolve=>test.held.push(resolve));
        test.completed.push(args.query);
        return response;
      }
      if(command==="usage_call_activity"){
        if(Object.keys(args.query).some(key=>!["observationId","start","end","cursor"].includes(key))) throw "invalidQuery";
        test.activity.push(args.query);
        const call=calls.find(call=>call.id===args.query.observationId);
        return {events:[{id:`activity-${call.id}`,time:call.time,kind:"custom_tool_call",label:"functions.exec · two commands",association:"logOrder",parentId:null,status:"completed",paths:["src/main.rs"],pathsInferred:true,fields:[{label:"Output",preview:"<img src=x onerror=alert(1)>",textRef:`text-${call.id}`}]}],nextCursor:null,scannedBytes:100,totalBytes:100,complete:true,association:"logOrder",notices:[]};
      }
      if(command==="usage_activity_text") { test.text.push(args.query);return args.query.offset?{text:"\nSecond chunk: full result",nextOffset:null}:{text:"<img src=x onerror=alert(1)>\nFirst chunk",nextOffset:50}; }
      return native(command,args);
    };
  });
  await page.goto("http://127.0.0.1:1425");
  await page.locator(".call-card").first().waitFor();
  assert.equal(await page.locator(".call-card").count(),50);
  const firstCallSummary = await page.locator(".call-card > summary").first().innerText();
  assert.match(firstCallSummary, /Uncached input: 20/);
  assert.match(firstCallSummary, /Cached input: 80/);
  assert.match(firstCallSummary, /Output · includes reasoning: 20/);
  assert.match(firstCallSummary, /Response: response-1/);
  assert.match(await page.locator(".call-timeline > .call-table-wrap").innerText(),/0\.01408/);
  await page.getByRole("button",{name:"Load next 50 calls"}).click();
  await page.waitForFunction(()=>document.querySelectorAll(".call-card").length===55);
  assert.match(await page.locator(".call-timeline > .call-table-wrap").innerText(),/0\.01408/,"Page totals do not become the last page subtotal");
  await page.locator(".call-card > summary").first().click();
  await page.locator(".activity-text > summary").first().click();
  await page.getByRole("button",{name:"Read more text"}).click();
  await page.getByText("Second chunk: full result",{exact:false}).waitFor();
  assert.equal(await page.locator(".activity-text img").count(),0,"Log text is never interpreted as HTML");
  assert.equal(await page.locator(".call-card").first().getByText("This invocation",{exact:true}).count(),1);
  if(process.env.TRACKER_UI_SCREENSHOT) await page.locator(".call-timeline").screenshot({path:process.env.TRACKER_UI_SCREENSHOT});

  async function drag(selector,from,to,modifier){
    const target=page.locator(`${selector} .u-over`).first();await target.scrollIntoViewIfNeeded();const box=await target.boundingBox();assert.ok(box);
    if(modifier) await page.keyboard.down(modifier);
    await page.mouse.move(box.x+box.width*from,box.y+box.height*.4);await page.mouse.down();await page.mouse.move(box.x+box.width*to,box.y+box.height*.5,{steps:8});await page.mouse.up();
    if(modifier) await page.keyboard.up(modifier);
  }
  async function lastWindow(){return page.evaluate(()=>window.dashboardTest.calls.at(-1));}
  await drag(".dashboard-plot",.7,.3);
  await page.waitForFunction(()=>window.dashboardTest.calls.at(-1).range==="custom"&&document.querySelectorAll(".call-card").length>0);
  let selected=await lastWindow();assert.ok(selected.start.seconds<selected.end.seconds);
  let callQuery=await page.evaluate(()=>window.callsTest.requests.at(-1));assert.deepEqual(callQuery.start,selected.start);assert.deepEqual(callQuery.end,selected.end);
  const firstWidth=selected.end.seconds-selected.start.seconds;
  await drag(".usage-chart-tokens",.2,.8);
  await page.waitForFunction(width=>{const query=window.dashboardTest.calls.at(-1);return query.range==="custom"&&query.end.seconds-query.start.seconds<width;},firstWidth);
  const second=await lastWindow();
  await page.getByRole("button",{name:"Back",exact:true}).click();
  await page.waitForFunction(expected=>JSON.stringify(window.dashboardTest.calls.at(-1).start)===JSON.stringify(expected),selected.start);
  const beforePan=await lastWindow();
  await drag(".usage-chart-cost",.4,.6,"Shift");
  await page.waitForFunction(old=>window.dashboardTest.calls.at(-1).start.seconds!==old,beforePan.start.seconds);
  selected=await lastWindow();assert.ok(Math.abs((selected.end.seconds-selected.start.seconds)-firstWidth)<=1,"Pan preserves duration");
  const plot=page.locator(".dashboard-plot .u-over");await plot.scrollIntoViewIfNeeded();const box=await plot.boundingBox();await page.mouse.move(box.x+box.width*.5,box.y+box.height*.4);
  await page.keyboard.down("Control");await page.mouse.wheel(0,-180);await page.keyboard.up("Control");
  await page.waitForFunction(width=>{const q=window.dashboardTest.calls.at(-1);return q.end.seconds-q.start.seconds<width;},firstWidth);
  const beforeEscape=await page.evaluate(()=>window.dashboardTest.calls.length);
  await plot.scrollIntoViewIfNeeded();const current=await plot.boundingBox();await page.mouse.move(current.x+20,current.y+30);await page.mouse.down();await page.mouse.move(current.x+180,current.y+30);await page.keyboard.press("Escape");await page.mouse.up();
  assert.equal(await page.evaluate(()=>window.dashboardTest.calls.length),beforeEscape,"Escape cancels selection");
  await page.getByRole("button",{name:"Reset zoom"}).click();await page.waitForFunction(()=>window.dashboardTest.calls.at(-1).range==="last7Days");
  await page.locator(".period-controls > details > summary").click();
  const from="2026-01-02T02:00",through="2026-01-02T01:00";
  await page.getByLabel("Start",{exact:true}).fill(from);await page.getByLabel("End",{exact:true}).fill(through);await page.getByRole("button",{name:"Apply dates"}).click();await page.locator("#period-error").waitFor();
  assert.equal(await page.getByLabel("Start",{exact:true}).inputValue(),from,"Invalid dates remain entered");
  const dates=await page.evaluate(()=>{
    const local=seconds=>{const date=new Date(seconds*1000);return new Date(date.getTime()-date.getTimezoneOffset()*60000).toISOString().slice(0,16);};
    const start=local(window.callsTest.calls[10].time.seconds),end=local(window.callsTest.calls[20].time.seconds);
    return {start,end,startSeconds:Date.parse(start)/1000,endSeconds:Date.parse(end)/1000};
  });
  await page.getByLabel("Start",{exact:true}).fill(dates.start);await page.getByLabel("End",{exact:true}).fill(dates.end);await page.getByRole("button",{name:"Apply dates"}).click();
  await page.waitForFunction(expected=>{const q=window.dashboardTest.calls.at(-1);return q.range==="custom"&&q.start.seconds===expected.startSeconds&&q.end.seconds===expected.endSeconds;},dates);
  const fixed=await lastWindow();
  const refreshCount=await page.evaluate(()=>{window.dashboardTest.callbacks.broadcast({payload:{}});return window.dashboardTest.calls.length;});
  await page.waitForFunction(count=>window.dashboardTest.calls.length>count,refreshCount);
  assert.deepEqual((await lastWindow()).start,fixed.start);assert.deepEqual((await lastWindow()).end,fixed.end,"Live refresh does not move a fixed interval");
  await page.getByLabel("Last",{exact:true}).fill("48");await page.getByLabel("Duration unit",{exact:true}).selectOption("3600");await page.getByRole("button",{name:"Follow this period"}).click();await page.waitForFunction(()=>window.dashboardTest.calls.at(-1).durationSeconds===172800);
  await page.getByRole("button",{name:"7 days",exact:true}).click();await page.waitForFunction(()=>window.dashboardTest.calls.at(-1).range==="last7Days"&&document.querySelectorAll(".call-card").length===50);
  await page.getByLabel("Model ID",{exact:true}).fill("model-b");await page.getByRole("button",{name:"Filter calls"}).click();await page.waitForFunction(()=>document.querySelectorAll(".call-card").length===27);
  assert.match(await page.locator(".call-timeline > .call-table-wrap").innerText(),/Filtered calls/);
  assert.equal(await page.locator(".call-model").filter({hasText:"model-a"}).count(),0);
  const completed=await page.evaluate(()=>{window.callsTest.hold=true;return window.callsTest.completed.length;});
  await page.getByLabel("Model ID",{exact:true}).fill("model-a");await page.getByRole("button",{name:"Filter calls"}).click();await page.waitForFunction(()=>window.callsTest.held.length===1);
  await page.getByLabel("Model ID",{exact:true}).fill("model-b");await page.getByRole("button",{name:"Filter calls"}).click();await page.waitForFunction(()=>window.callsTest.held.length===2);
  await page.evaluate(()=>window.callsTest.held.pop()());await page.waitForFunction(()=>document.querySelectorAll(".call-card").length===27);
  await page.evaluate(()=>{window.callsTest.hold=false;window.callsTest.held.pop()();});await page.waitForFunction(count=>window.callsTest.completed.length===count+2,completed);
  assert.equal(await page.locator(".call-model").filter({hasText:"model-a"}).count(),0,"A stale invocation response never replaces the latest filters");
  await page.setViewportSize({width:390,height:900});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,"Narrow dashboard has no horizontal overflow");
  assert.deepEqual(failures,[]);
  console.log("Call inspection UI: gestures on all three charts, shared ranges, undo/reset, fixed/trailing periods, validation, call filters, paging, exact totals, text paging, escaped content and narrow layout passed.");
} finally {await browser?.close();server.kill();}
