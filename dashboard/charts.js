import { createChart } from 'lightweight-charts';

export const renderChart = (data, container) => {
  const chart = createChart(container, { width: 800, height: 400 });
  const lineSeries = chart.addLineSeries();
  lineSeries.setData(data);
};
