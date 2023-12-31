interface IHystogramFragment {
    start: number,
    end: number,
    count: number,
}

interface ILimits {
    UpperLimit: number,
    LowerLimit: number,
    Target: number,
}

interface BoxPlot {
    median: number,
    q1: number,
    q3: number,
    iqr: number,
    lower_bound: number,
    upper_bound: number,
}

interface IAutoAdjustReport {
    DisplayBoxes: BoxPlot[],
    Hystogramm: IHystogramFragment[],
    Limits: ILimits,
}

// -- for chartjs-chart-box-and-violin-plot --

interface IBaseItem {
    min: number;
    median: number;
    max: number;
    /**
     * values of the raw items used for rendering jittered background points
     */
    items?: number[];
}

interface IBoxPlotItem extends IBaseItem {
    q1: number;
    q3: number;
    whiskerMin?: number;
    whiskerMax?: number;
    /**
     * list of box plot outlier values
     */
    outliers?: number[];
}

// on page loaded jquery
$(() => {
    // https://www.chartjs.org/docs/2.9.4/getting-started/integration.html#content-security-policy
    Chart.platform.disableCSSInjection = true;

    // https://getbootstrap.com/docs/4.0/components/tooltips/
    $('[data-toggle="tooltip"]').tooltip()

    $('#rezonators').on('click', 'tbody tr', (ev) => {
        let channel_to_select: number;
        if (ev.target.tagName === 'TD' || ev.target.tagName === 'TH') {
            // <td> / <th>
            channel_to_select = parseInt($(ev.target)
                .parent()
                .attr('row-index')) - 1;
        } else if ((ev.target.tagName === 'CODE') || (ev.target.tagName === 'svg')) {
            // <td><code></td> / <td><svg></td>
            channel_to_select = parseInt($(ev.target)
                .parent()
                .parent()
                .attr('row-index')) - 1;
        } else if (ev.target.tagName === 'path') {
            // <td><svg><path></svg></td>
            channel_to_select = parseInt($(ev.target)
                .parent()
                .parent()
                .parent()
                .attr('row-index')) - 1;
        }

        select_row_table_a(channel_to_select);
    });
});


function select_row_table_a(rez: number): void {
    const primary_class = 'bg-primary';
    const newly_selected = $('#rez-' + (rez + 1).toString() + '-row');
    if (!newly_selected.hasClass(primary_class)) {
        newly_selected.addClass(primary_class).siblings().removeClass(primary_class);
    }

    $.ajax({
        url: '/stat_auto/' + rez,
        method: 'GET',
        contentType: 'application/json',
        success: (data: IAutoAdjustReport) => {
            plot_history_a(data.DisplayBoxes, data.Limits);
            plot_hystogramm_a(data.Hystogramm);
        }
    })
}

function plot_history_a(boxes: BoxPlot[], limits: ILimits) {
    const total_points = boxes.length;

    let labels: Array<string> = [];
    for (var i = 0; i < total_points; ++i) {
        labels.push((i + 1).toString())
    }

    const datasets = [
        { // 0
            type: 'line',
            label: 'Upper Limit',
            lineTension: 0,
            pointRadius: 0,
            fill: 'top',
            backgroundColor: 'rgba(240, 81, 81, 0.5)',
            borderColor: 'rgb(240, 81, 81)',
            data: Array<number>(total_points).fill(limits.UpperLimit),
        },
        { // 1
            type: 'line',
            label: 'Lower Limit',
            lineTension: 0,
            pointRadius: 0,
            fill: 'bottom', // заполнить область до графика 1
            backgroundColor: 'rgba(204, 167, 80, 0.5)',
            borderColor: 'rgb(204, 167, 80)',
            data: Array<number>(total_points).fill(limits.LowerLimit),
        },
        { // 2
            type: 'line',
            label: 'Target',
            lineTension: 0,
            pointRadius: 0,
            fill: false,
            borderColor: 'rgb(8, 150, 38)',
            data: Array<number>(total_points).fill(limits.Target),
        },
        { // 3
            label: 'Freq',
            backgroundColor: "rgba(0,23,230,0.5)",
            borderColor: "rgb(0,23,230)",
            borderWidth: 1,
            outlierColor: "#999999",
            padding: 10,
            itemRadius: 0,
            data:
                boxes.map(b => ({
                    min: b.lower_bound,
                    median: b.median,
                    max: b.upper_bound,
                    q1: b.q1,
                    q3: b.q3,
                })) as IBoxPlotItem[] as any[],
        }
    ];

    const config = {
        type: 'boxplot',
        data: {
            datasets: datasets,
            labels: labels,
        },
        options: {
            tooltips: {
                enabled: false
            },
            responsive: true,
            aspectRatio: 2.3,
            legend: {
                display: false,
            },
            scales: {
                xAxes: [{
                    display: false,
                    ticks: {
                        display: false
                    }
                }],
            }
        }
    };

    new Chart(
        $('#adj-history-plot').get()[0] as HTMLCanvasElement, config);
}

function plot_hystogramm_a(hysto_data: IHystogramFragment[]) {
    const config = {
        type: 'bar',
        data: {
            labels: hysto_data.map((h) => round_to_2_digits(h.end)),
            datasets: [{
                label: 'Перестройка, Гц',
                data: hysto_data.map((h) => h.count),
                backgroundColor: 'rgba(240, 81, 81, 0.4)',
                borderColor: 'rgba(240, 81, 81, 1)',
                borderWidth: 1,
            }]
        },
        options: {
            tooltips: {
                enabled: false
            },
            responsive: true,
            aspectRatio: 2.3,
            legend: {
                display: false,
            },
            scales: {
                yAxes: [{
                    ticks: {
                        beginAtZero: true
                    }
                }]
            }
        }
    };

    new Chart(
        $('#hystogramm-plot').get()[0] as HTMLCanvasElement, config);
}
