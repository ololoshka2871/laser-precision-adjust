interface IDataPoint {
    x: number,
    y: number,
}

interface IDisplayFragment {
    points: IDataPoint[],
    color_code_rgba: string,
}

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

interface IAdjustReport {
    DisplayFragments: IDisplayFragment[],
    Hystogramm: IHystogramFragment[],
    Limits: ILimits,
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

        select_row_table(channel_to_select);
    });

    // report
    $('#gen-report').on('click', (ev) => {
        let report_id = prompt('Введите номер партии:');
        gen_report(report_id);
    });
});


function select_row_table(rez: number): void {
    const primary_class = 'bg-primary';
    const newly_selected = $('#rez-' + (rez + 1).toString() + '-row');
    if (!newly_selected.hasClass(primary_class)) {
        newly_selected.addClass(primary_class).siblings().removeClass(primary_class);
    }

    $.ajax({
        url: '/stat/' + rez,
        method: 'GET',
        contentType: 'application/json',
        success: (data: IAdjustReport) => {
            plot_history(data.DisplayFragments, data.Limits);
            plot_hystogramm(data.Hystogramm);
        }
    })
}

function plot_history(fragments: IDisplayFragment[], limits: ILimits) {
    const total_points = fragments.reduce((a, f) => a + f.points.length, 0);

    const labels = [];
    const datasets = [
        { // 0
            label: 'Upper Limit',
            lineTension: 0,
            pointRadius: 0,
            fill: 'top',
            backgroundColor: 'rgba(240, 81, 81, 0.5)',
            borderColor: 'rgb(240, 81, 81)',
            data: Array<number>(total_points).fill(limits.UpperLimit),
        },
        { // 1
            label: 'Lower Limit',
            lineTension: 0,
            pointRadius: 0,
            fill: 'bottom', // заполнить область до графика 1
            backgroundColor: 'rgba(204, 167, 80, 0.5)',
            borderColor: 'rgb(204, 167, 80)',
            data: Array<number>(total_points).fill(limits.LowerLimit),
        },
        { // 2
            label: 'Target',
            lineTension: 0,
            pointRadius: 0,
            fill: false,
            borderColor: 'rgb(8, 150, 38)',
            data: Array<number>(total_points).fill(limits.Target),
        }
    ];
    for (const fragment of fragments) {
        const before_len = labels.length;
        labels.push(...fragment.points.map((p) => p.x));

        const data = Array<number>(before_len).fill(NaN);
        data.push(...fragment.points.map((p) => p.y));

        datasets.push({
            label: null,
            lineTension: 0,
            pointRadius: 0,
            fill: 'false',
            backgroundColor: null,
            borderColor: fragment.color_code_rgba,
            data: data,
        })
    }

    const config = {
        type: 'line',
        data: {
            labels: labels,
            datasets: datasets
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
    }

    new Chart(
        $('#adj-history-plot').get()[0] as HTMLCanvasElement, config);
}

function plot_hystogramm(hysto_data: IHystogramFragment[]) {
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