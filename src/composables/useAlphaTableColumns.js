import { h, computed } from "vue";
import { NPopover, NTag } from "naive-ui";
import VChart from "vue-echarts";
import { use } from "echarts/core";
import { LineChart } from "echarts/charts";
import { GridComponent, TooltipComponent, LegendComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import Prism from "prismjs";
import "prismjs/themes/prism.css";
// Support python-like syntax fit for Alpha expressions
import "prismjs/components/prism-python";

// Register ECharts components
use([LineChart, GridComponent, TooltipComponent, CanvasRenderer, LegendComponent]);

// EChart options generator
function getPNLOption(pnlData) {
  if (!pnlData) return {};

  // Check if pnlData is an object with children_series (RA Parent multi-child view)
  const isMulti = pnlData?.children_series && pnlData.children_series.length > 0;

  if (isMulti) {
    const totalSeries = pnlData.pnl_series || [];
    const dates = totalSeries.map((p) => p.date);

    const seriesList = [];

    const childColors = [
      "#2563eb", "#f97316", "#16a34a", "#8b5cf6", "#ec4899",
      "#06b6d4", "#eab308", "#14b8a6", "#6366f1", "#84cc16"
    ];

    pnlData.children_series.forEach((c, idx) => {
      const color = childColors[idx % childColors.length];
      const dateValDict = {};
      c.pnl_series.forEach((p) => {
        dateValDict[p.date] = p.pnl;
      });
      const alignedData = dates.map((d) => (dateValDict[d] !== undefined ? dateValDict[d] : null));

      seriesList.push({
        name: `${c.region || c.id}`,
        data: alignedData,
        type: "line",
        symbol: "none",
        lineStyle: { color, width: 1.8 },
      });
    });

    return {
      grid: { left: 50, right: 20, top: 40, bottom: 40, containLabel: true },
      tooltip: {
        trigger: "axis",
        formatter: (params) => {
          if (!params || !params.length) return "";
          let tip = `<div style="font-weight:bold;margin-bottom:4px;">${params[0].axisValue}</div>`;
          params.forEach((item) => {
            if (item.value != null) {
              const val = Number(item.value).toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
              tip += `<div style="display:flex;justify-content:space-between;gap:12px;">
                <span>${item.marker} ${item.seriesName}</span>
                <span style="font-weight:600;">${val}</span>
              </div>`;
            }
          });
          return tip;
        },
      },
      legend: {
        show: true,
        type: "scroll",
        top: 4,
        textStyle: { fontSize: 11 },
      },
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
      series: seriesList,
    };
  }

  // Single series (regular / child alpha)
  const series = Array.isArray(pnlData) ? pnlData : (pnlData?.pnl_series || []);
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

function renderParentMetricCell(row, formattedVal) {
  if (!formattedVal || formattedVal === "--") return "--";
  if (row.alpha_type === "RA_PARENT") {
    return h("span", { style: { fontWeight: "600", color: "#1976d2" } }, formattedVal);
  }
  return formattedVal;
}

// The Composable function
export function useAlphaTableColumns({ expandedRowIds, pnlDataMap, loadingSet, loadPNL, visibleColumns }) {
  const allColumns = [
    {
      title: "Category",
      key: "category",
      width: 120,
      render: (row) => {
        const pyramidCheck = row.is?.checks?.find(c => c.name === 'MATCHES_PYRAMID');
        if (pyramidCheck && pyramidCheck.pyramid) {
          return pyramidCheck.pyramid.split('/').pop();
        }
        return row.self_category?.join(", ") || "-";
      }
    },
    {
      title: "Settings",
      key: "settings",
      width: 150,
      ellipsis: true,
      render(row) {
        const key = "set-" + row.id;
        const isExpanded = expandedRowIds.value.has(key);
        const settings = row.settings || {};
        
        const settingsText = Object.entries(settings)
          .map(([k, v]) => `${k}: ${v}`)
          .join("\n");

        return h(
          "div",
          {
            class: ["regular-cell", isExpanded ? "expanded" : ""],
            style: { cursor: "pointer" },
            onClick: () => {
              if (window.getSelection().toString()) return;
              isExpanded ? expandedRowIds.value.delete(key) : expandedRowIds.value.add(key);
            },
          },
          settingsText || "--"
        );
      },
    },
    {
      title: "Regular",
      key: "code",
      width: 300,
      render(row) {
        const isExpanded = expandedRowIds.value.has(row.id);
        const code = row.code || "--";
        
        if (isExpanded) {
          // Use Prism to highlight when expanded
          const highlighted = Prism.highlight(code, Prism.languages.python, "python");
          return h(
            "div",
            {
              class: ["regular-cell", "expanded"],
              style: { cursor: "pointer" },
              onClick: () => {
                if (window.getSelection().toString()) return;
                expandedRowIds.value.delete(row.id);
              },
              innerHTML: highlighted
            }
          );
        }

        return h(
          "div",
          {
            class: ["regular-cell"],
            style: { cursor: "pointer" },
            onClick: () => {
              if (window.getSelection().toString()) return;
              expandedRowIds.value.add(row.id);
            },
          },
          code
        );
      },
    },
    {
      title: "ID",
      key: "id",
      tree: true,
      render(row) {
        const isParent = row.alpha_type === "RA_PARENT";
        const childCount = row.children?.length || 0;

        return h(
          "div",
          { style: { display: "inline-flex", alignItems: "center" } },
          [
            h(
              "span",
              {
                style: {
                  fontFamily: "monospace",
                  fontWeight: isParent ? "bold" : "normal",
                  color: isParent ? "#1976d2" : "inherit",
                },
              },
              row.id
            ),
            isParent && childCount > 0
              ? h(
                  "span",
                  {
                    style: {
                      marginLeft: "4px",
                      color: "#1976d2",
                      fontWeight: "600",
                    },
                  },
                  `(${childCount})`
                )
              : null,
          ]
        );
      },
    },
    {
      title: "Region",
      key: "region",
      render(row) {
        if (row.alpha_type === "RA_PARENT") {
          return h(
            NTag,
            { size: "tiny", type: "warning", round: true, bordered: false },
            { default: () => "ALL" }
          );
        }
        return row.region || "--";
      },
    },
    {
      title: "Score",
      key: "pnl_score",
      sorter: true,
      render: (row) =>
        renderParentMetricCell(
          row,
          row.pnl_score != null ? Math.round(row.pnl_score).toLocaleString() : "--"
        ),
    },
    {
      title: "IS Score",
      key: "is_score",
      sorter: true,
      render: (row) =>
        renderParentMetricCell(
          row,
          row.is_score != null ? Math.round(row.is_score).toLocaleString() : "--"
        ),
    },
    { 
      title: "Sharpe", 
      key: "sharpe", 
      sorter: true,
      render: (row) =>
        renderParentMetricCell(
          row,
          row.sharpe != null ? Number(row.sharpe).toFixed(2) : "--"
        ),
    },
    { 
      title: "Fitness", 
      key: "fitness", 
      sorter: true,
      render: (row) =>
        renderParentMetricCell(
          row,
          row.fitness != null ? Number(row.fitness).toFixed(2) : "--"
        ),
    },
    { 
      title: "OS Sharpe", 
      key: "os_sharpe", 
      sorter: true,
      render: (row) =>
        renderParentMetricCell(
          row,
          row.os_sharpe != null ? Number(row.os_sharpe).toFixed(2) : "--"
        ),
    },
    { 
      title: "OS Fitness", 
      key: "os_fitness", 
      sorter: true,
      render: (row) =>
        renderParentMetricCell(
          row,
          row.os_fitness != null ? Number(row.os_fitness).toFixed(2) : "--"
        ),
    },
    {
      title: "Returns",
      key: "returns",
      sorter: true,
      render: (row) =>
        renderParentMetricCell(
          row,
          row.returns != null ? (row.returns * 100).toFixed(2) + "%" : "--"
        ),
    },
    {
      title: "Turnover",
      key: "turnover",
      sorter: true,
      render: (row) =>
        renderParentMetricCell(
          row,
          row.turnover != null ? (row.turnover * 100).toFixed(2) + "%" : "--"
        ),
    },
    {
      title: "Margin",
      key: "margin",
      sorter: true,
      render: (row) =>
        renderParentMetricCell(
          row,
          row.margin != null ? (row.margin * 10000).toFixed(2) + "‱" : "--"
        ),
    },
    { 
      title: "Drawdown", 
      key: "drawdown", 
      sorter: true,
      render: (row) =>
        renderParentMetricCell(
          row,
          row.drawdown != null ? (row.drawdown * 100).toFixed(2) + "%" : "--"
        ),
    },
    {
      title: "Sub-U Sharpe",
      key: "sub_universe_sharpe",
      sorter: true,
      width: 70,
      render: (row) =>
        renderParentMetricCell(
          row,
          row.sub_universe_sharpe != null ? Number(row.sub_universe_sharpe).toFixed(2) : "--"
        ),
    },
    {
      title: "Date Created",
      key: "date_created",
      render: (row) => row.date_created?.split("T")[0] ?? "--",
      sorter: true,
    },
    {
      title: "Message",
      key: "message",
      width: 320,
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
      width: 110,
      render(row) {
        const isParent = row.alpha_type === "RA_PARENT";
        return h(
          NPopover,
          { trigger: "hover", width: isParent ? 680 : 600 },
          {
            trigger: () => {
              return h(
                "span",
                {
                  style: {
                    cursor: "pointer",
                    color: isParent ? "#2563eb" : "#3b82f6",
                    fontWeight: isParent ? "bold" : "normal",
                  },
                  onmouseenter: () => {
                    if (!pnlDataMap.value[row.id] && !loadingSet.value.has(row.id)) {
                      loadPNL(row.id);
                    }
                  },
                },
                isParent ? "📈 查看 (全部)" : "📈 查看"
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
      render: (row) => renderParentMetricCell(row, row.corr_ppac ?? "-"),
    },
    {
      title: "Corr os",
      key: "corr_os",
      render: (row) => renderParentMetricCell(row, row.corr_os ?? "-"),
    },
    {
      title: "Corr Prod",
      key: "corr_prod",
      render: (row) => row.currentProdCorrelation?.value ?? "-",
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
      if (col.key === 'message') return visibleColumns.value.includes('message');
      if (col.key === 'pnl') return visibleColumns.value.includes('pnl');
      if (col.key === 'corr_ppac' || col.key === 'corr_os') return visibleColumns.value.includes('corr');
      if (col.key === 'corr_prod') return visibleColumns.value.includes('corr_prod');
      if (col.key === 'category') return visibleColumns.value.includes('category');
      if (col.key === 'settings') return visibleColumns.value.includes('settings');
      return true;
    });
  });

  return { columns };
}

