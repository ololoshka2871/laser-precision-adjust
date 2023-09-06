// define interface for jquery JQuery<HTMLElement> where method tooltip is defined
interface JQuery<TElement extends Element = HTMLElement> extends Iterable<TElement> {
    tooltip(options?: any): JQuery<TElement>;
}

// declare oboe as defined global function
declare function oboe(url: string): any;

// declare hotkeys as defined global function
declare function hotkeys(key: string, callback: (event: KeyboardEvent, handler: any) => void): void;

// ---------------------------------------------------------------------------------------------

const POINTS_ON_PLOT = 100;

// on page loaded jquery
$(() => {
    // https://www.chartjs.org/docs/2.9.4/getting-started/integration.html#content-security-policy
    Chart.platform.disableCSSInjection = true;

    // https://getbootstrap.com/docs/4.0/components/tooltips/
    $('[data-toggle="tooltip"]').tooltip()

    $('#rezonators').on('click', 'tbody tr', (ev) => {
        const channel_to_select = parseInt($(ev.target).parent().attr('row-index')) - 1;
        $.ajax({
            url: '/control/select',
            method: 'POST',
            data: JSON.stringify({ Channel: channel_to_select }),
            contentType: 'application/json',
            success: (data) => {
                if (data.success) {
                    noty_success('Выбран резонатор №' + (channel_to_select + 1).toString());
                } else {
                    noty_error('Ошибка: ' + data.error);
                }
            }
        })
    });

    $('.camera-ctrl').on('click', (ev) => {
        const action = $(ev.target).attr('ctrl-request');
        $.ajax({
            url: '/control/camera',
            method: 'POST',
            data: JSON.stringify({ CameraAction: action }),
            contentType: 'application/json',
            success: (data) => {
                if (!data.success) {
                    noty_error('Ошибка: ' + data.error);
                }
            }
        })
    });

    const chart = new Chart(
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
            const angle = state.TimesTamp;
            const current_freq = state.CurrentFreq;

            const target = state.TargetFreq;
            const offset_hz = state.WorkOffsetHz;
            const upperLimit = target + offset_hz;
            const lowerLimit = target - offset_hz;

            // если длина массива больше POINTS_ON_PLOT, то удаляем первый элемент
            if (chart.data.labels.length > POINTS_ON_PLOT) {
                chart.data.labels.shift();
                chart.data.datasets.forEach(ds => ds.data.shift());
            }

            // добавляем новые значения в график
            chart.data.labels.push(angle);
            chart.data.datasets[0].data.push(upperLimit);
            chart.data.datasets[1].data.push(lowerLimit);
            chart.data.datasets[2].data.push(current_freq);
            chart.data.datasets[3].data.push(target);
            chart.update();

            update_f_re_display({
                freq: current_freq,
                min: lowerLimit,
                max: upperLimit
            });

            select_rezonator(state.SelectedChannel);
        });

    // hotkeys
    hotkeys('space', (event, handler) => {
        console.log(handler.key + ' pressed');
        event.preventDefault();
    });
});

function update_f_re_display(cfg): void {
    const value = Math.round(cfg.freq * 100) / 100;
    $('#current-freq-display').text(value);

    const bg_class = value < cfg.min
        ? 'bg-warning'
        : (value > cfg.max ? 'bg-danger' : 'bg-success');

    const display_bg = $('#current-freq-display-bg');
    if (!display_bg.hasClass(bg_class)) {
        display_bg.removeClass('bg-success bg-danger bg-warning').addClass(bg_class);
    }
}

function select_rezonator(channel: number): void {
    const primary_class = 'bg-primary';
    const newly_selected = $('#rez-' + (channel + 1).toString() + '-row');
    if (!newly_selected.hasClass(primary_class)) {
        newly_selected.addClass(primary_class).siblings().removeClass(primary_class);
    }
}