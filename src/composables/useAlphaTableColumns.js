import { h, computed } from "vue";
import { NPopover } from "naive-ui";
import VChart from "vue-echarts";
import { use } from "echarts/core";
import { LineChart } from "echarts/charts";
import { GridComponent, TooltipComponent, LegendComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";

// Register ECharts components
use([LineChart, GridComponent, TooltipComponent, CanvasRenderer, LegendComponent]);

// EChart options generator
function getPNLOption(series) {
  const dates = series.map((p) => p.date);
  const pnlValues = series.map((p) => p.pnl);
  const riskNeutralizedPnL = series.map((p) => p.risk_neutralized_pnl);

  return {
    grid: { left: 50, right: 20, top: 30, bottom: 40, containLabel: true },
    tooltip: { trigger: "axis" },
    legend: { show: true },
    xAxis: {
      type: "category",
      data: dates,
      position: "bottom",
      boundaryGap: false,
      axisLine: { onZero: false, lineStyle: { color: "#ccc" } },
      axisLabel: { rotate: 0, fontSize: 10, formatter: (value) => value.slice(0, 4) },
    },
    yAxis: {
      type: "value",
      min: "dataMin",
      max: "dataMax",
      axisLabel: {
        fontSize: 10,
        formatter: (value) => {
          const absVal = Math.abs(value);
          if (absVal >= 1e7) return (value / 1e6).toFixed(1) + "M";
          if (absVal >= 1e4) return (value / 1e3).toFixed(1) + "K";
          return value.toFixed(1);
        },
      },
      splitLine: { lineStyle: { color: "#eee" } },
    },
    series: [
      { name: "PnL", data: pnlValues, type: "line", symbol: "none", lineStyle: { color: "#81CDD6", width: 1.5 } },
      { name: "Risk Neutralized PnL", data: riskNeutralizedPnL, type: "line", symbol: "none", lineStyle: { color: "#7CC873", width: 1.5 } },
    ],
  };
}

// The Composable function
export function useAlphaTableColumns({ expandedRowIds, pnlDataMap, loadingSet, loadPNL, visibleColumns }) {
  const allColumns = [
    {
      title: "Regular",
      key: "code",
      render(row) {
        const isExpanded = expandedRowIds.value.has(row.id);
        return h(
          "div",
          {
            class: ["regular-cell", isExpanded ? "expanded" : ""],
            style: { cursor: "pointer" },
            onClick: () => {
              if (window.getSelection().toString()) return;
              isExpanded ? expandedRowIds.value.delete(row.id) : expandedRowIds.value.add(row.id);
            },
          },
          row.code || "--"
        );
      },
    },
    { title: "ID", key: "id" },
    { title: "Region", key: "region" },
    {
      title: "Universe",
      key: "universe",
      render(row) {
        const key = "univ-" + row.id;
        const isExpanded = expandedRowIds.value.has(key);
        return h(
          "div",
          {
            class: ["regular-cell", isExpanded ? "expanded" : ""],
            style: { cursor: "pointer" },
            onClick: () => {
              if (window.getSelection().toString()) return;
              isExpanded ? expandedRowIds.value.delete(row.id) : expandedRowIds.value.add(key);
            },
          },
          row.universe || "--"
        );
      },
    },
    {
      title: "Neutralization",
      key: "neutralization",
      render: (row) => row.neutralization || "--",
    },
    {
      title: "Score",
      key: "pnl_score",
      sorter: true,
      render: (row) => (row.pnl_score != null ? Math.round(row.pnl_score).toLocaleString() : "--"),
    },
    {
      title: "IS Score",
      key: "is_score",
      sorter: true,
      render: (row) => (row.is_score != null ? Math.round(row.is_score).toLocaleString() : "--"),
    },
    { 
      title: "Sharpe", 
      key: "sharpe", 
      sorter: true,
      render: (row) => (row.sharpe != null ? Number(row.sharpe).toFixed(2) : "--")
    },
    { 
      title: "Fitness", 
      key: "fitness", 
      sorter: true,
      render: (row) => (row.fitness != null ? Number(row.fitness).toFixed(2) : "--")
    },
    { 
      title: "OS Sharpe", 
      key: "os_sharpe", 
      sorter: true,
      render: (row) => (row.os_sharpe != null ? Number(row.os_sharpe).toFixed(2) : "--")
    },
    { 
      title: "OS Fitness", 
      key: "os_fitness", 
      sorter: true,
      render: (row) => (row.os_fitness != null ? Number(row.os_fitness).toFixed(2) : "--")
    },
    {
      title: "Returns",
      key: "returns",
      sorter: true,
      render: (row) => (row.returns != null ? (row.returns * 100).toFixed(2) + "%" : "--"),
    },
    {
      title: "Turnover",
      key: "turnover",
      sorter: true,
      render: (row) => (row.turnover != null ? (row.turnover * 100).toFixed(2) + "%" : "--"),
    },
    {
      title: "Margin",
      key: "margin",
      sorter: true,
      render: (row) => (row.margin != null ? (row.margin * 10000).toFixed(2) + "‱" : "--"),
    },
    { 
      title: "Drawdown", 
      key: "drawdown", 
      sorter: true,
      render: (row) => (row.drawdown != null ? (row.drawdown * 100).toFixed(2) + "%" : "--")
    },
    { title: "Sub-U Sharpe", key: "sub_universe_sharpe", sorter: true, width: 70 },
    {
      title: "Date Created",
      key: "date_created",
      render: (row) => row.date_created?.split("T")[0] ?? "--",
      sorter: true,
    },
    {
      title: "Message",
      key: "message",
      render(row) {
        const key = "msg-" + row.id;
        const isExpanded = expandedRowIds.value.has(key);
        return h(
          "div",
          {
            class: ["message-cell", isExpanded ? "expanded" : ""],
            style: { cursor: "pointer" },
            onClick: () => {
              if (window.getSelection().toString()) return;
              isExpanded ? expandedRowIds.value.delete(key) : expandedRowIds.value.add(key);
            },
          },
          row.message || "--"
        );
      },
    },
    {
      title: "PnL",
      key: "pnl",
      width: 100,
      render(row) {
        return h(
          NPopover,
          { trigger: "hover", width: 600 },
          {
            trigger: () => {
              return h(
                "span",
                {
                  style: "cursor: pointer; color: #3b82f6",
                  onmouseenter: () => {
                    if (!pnlDataMap.value[row.id] && !loadingSet.value.has(row.id)) {
                      loadPNL(row.id);
                    }
                  },
                },
                "📈 查看"
              );
            },
            default: () => {
              if (loadingSet.value.has(row.id)) return h("div", "Loading...");
              if (pnlDataMap.value[row.id] === null) return h("div", "❌ 加载失败");
              if (!pnlDataMap.value[row.id]) return h("div", "Hover to load");

              return h("div", { style: "width: 100%; height: 400px;" }, [
                h(VChart, { option: getPNLOption(pnlDataMap.value[row.id]) }),
              ]);
            },
          }
        );
      },
    },
    {
      title: "Corr ppac",
      key: "corr_ppac",
      render: (row) => row.corr_ppac ?? "-",
    },
    {
      title: "Corr os",
      key: "corr_os",
      render: (row) => row.corr_os ?? "-",
    },
  ];

  const columns = computed(() => {
    if (!visibleColumns || !visibleColumns.value) return allColumns;

    return allColumns.filter(col => {
      // Logic for visibility
      if (col.key === 'pnl_score') return visibleColumns.value.includes('score');
      if (col.key === 'is_score') return visibleColumns.value.includes('is_score');
      if (col.key === 'drawdown') return visibleColumns.value.includes('drawdown');
      if (col.key === 'os_sharpe' || col.key === 'os_fitness') return visibleColumns.value.includes('os');
      if (col.key === 'universe') return visibleColumns.value.includes('universe');
      if (col.key === 'neutralization') return visibleColumns.value.includes('neutralization');
      if (col.key === 'message') return visibleColumns.value.includes('message');
      if (col.key === 'pnl') return visibleColumns.value.includes('pnl');
      if (col.key === 'corr_ppac' || col.key === 'corr_os') return visibleColumns.value.includes('corr');
      return true;
    });
  });

  return { columns };
}

