// define interface for jquery JQuery<HTMLElement> where method tooltip is defined
interface JQuery<TElement extends Element = HTMLElement> extends Iterable<TElement> {
    tooltip(options?: any): JQuery<TElement>;
}

// declare oboe as defined global function
declare function oboe(url: string): any;

// declare hotkeys as defined global function
declare function hotkeys(key: string, callback: (event: KeyboardEvent, handler: any) => void): void;

// ---------------------------------------------------------------------------------------------

// on page loaded jquery
$(() => {
    // https://www.chartjs.org/docs/2.9.4/getting-started/integration.html#content-security-policy
    Chart.platform.disableCSSInjection = true;

    // https://getbootstrap.com/docs/4.0/components/tooltips/
    $('[data-toggle="tooltip"]').tooltip()

    $('#rezonators').on('click', 'tbody tr', (ev) => {
        $(ev.target).parent().addClass('bg-primary').siblings().removeClass('bg-primary');
    });

    let chart = new Chart(
        $('#adj-plot').get()[0] as HTMLCanvasElement,
        {
            type: 'line',
            data: {
                labels: [],
                datasets: [
                    {
                        label: 'Upper Limit',
                        data: [],
                        pointRadius: 0,
                        fill: 'top',
                        backgroundColor: 'rgba(240, 81, 81, 0.5)',
                        borderColor: 'rgb(240, 81, 81)',
                    },
                    {
                        label: 'Lower Limit',
                        data: [],
                        pointRadius: 0,
                        fill: 'bottom', // заполнить область до графика 1
                        backgroundColor: 'rgba(204, 167, 80, 0.5)',
                        borderColor: 'rgb(204, 167, 80)',
                    },
                    {
                        label: 'Actual',
                        data: [],
                        pointRadius: 0,
                        fill: false,
                        borderColor: 'rgb(75, 148, 204)',
                    },
                    {
                        label: 'Target',
                        data: [],
                        pointRadius: 0,
                        fill: false,
                        borderColor: 'rgb(8, 150, 38)',
                    }
                ]
            },
            options: {
                animation: {
                    duration: 0
                },
                tooltips: {
                    enabled: false // <-- this option disables tooltips
                },
                responsive: true,
                aspectRatio: 1,
                legend: {
                    display: false,
                },
                scales: {
                    xAxes: [{
                        display: false, //this will remove all the x-axis grid lines
                        ticks: {
                            display: false //this will remove only the label
                        }
                    }],
                }
            }
        });

    oboe('/state')
        .node('!.', (state: any) => {
            // state - это весь JSON объект, который пришел с сервера
            let angle = state.angle;
            let value = state.value;

            // get target value from #freq-target input element if value is emty then use 0
            let v = $('#freq-target').val();
            let target: number = v ? parseFloat(v.toString()) : 0;

            let offset = 30 * 1e-6; // 30 ppm
            let upperLimit = target + target * offset;
            let lowerLimit = target - target * offset;

            // если длина массива больше 100, то удаляем первый элемент
            if (chart.data.labels.length > 100) {
                chart.data.labels.shift();
                chart.data.datasets.forEach(ds => ds.data.shift());
            }

            // добавляем новые значения в график
            chart.data.labels.push(angle);
            chart.data.datasets[0].data.push(upperLimit);
            chart.data.datasets[1].data.push(lowerLimit);
            chart.data.datasets[2].data.push(value);
            chart.data.datasets[3].data.push(target);
            chart.update();

            update_f_re_display({
                freq: value,
                min: lowerLimit,
                max: upperLimit
            })
        });

    // hotkeys
    hotkeys('space', (event, handler) => {
        console.log(handler.key + ' pressed');
        event.preventDefault();
    });
});

function update_f_re_display(cfg): void {
    let value = Math.round(cfg.freq * 100) / 100;
    $('#current-freq-display').text(value);

    let bg_class = value < cfg.min
        ? 'bg-warning'
        : (value > cfg.max ? 'bg-danger' : 'bg-success');

    let display_bg = $('#current-freq-display-bg');
    if (!display_bg.hasClass(bg_class)) {
        display_bg.removeClass('bg-success bg-danger bg-warning').addClass(bg_class);
    }
}