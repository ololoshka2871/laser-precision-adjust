// заглушки
declare var uv: any;

// define interface for jquery JQuery<HTMLElement> where method tooltip is defined
interface JQuery<TElement extends Element = HTMLElement> extends Iterable<TElement> {
    tooltip(options?: any): JQuery<TElement>;
}

// declare oboe as defined global function
declare function oboe(url: string): any;

// ---------------------------------------------------------------------------------------------

// on page loaded jquery
$(() => {
    // https://getbootstrap.com/docs/4.0/components/tooltips/
    $('[data-toggle="tooltip"]').tooltip()

    $('#rezonators').on('click', 'tbody tr', (ev) => {
        $(ev.target).parent().addClass('bg-primary').siblings().removeClass('bg-primary');
    });

    oboe('/state')
        .node('!.', (state: any) => { 
            // state - это весь JSON объект, который пришел с сервера
            console.log(state); 
        })

    const graphdef = {
        categories: ['uvCharts', 'Matisse', 'SocialByWay'],
        dataset: {
            'uvCharts': [
                { name: '2008', value: 16 },
                { name: '2009', value: 21 },
                { name: '2010', value: 43 },
                { name: '2011', value: 81 },
                { name: '2012', value: 105 },
                { name: '2013', value: 146 }
            ],
            'Matisse': [
                { name: '2008', value: 15 },
                { name: '2009', value: 28 },
                { name: '2010', value: 42 },
                { name: '2011', value: 88 },
                { name: '2012', value: 100 },
                { name: '2013', value: 143 }
            ],
            'SocialByWay': [
                { name: '2008', value: 17 },
                { name: '2009', value: 29 },
                { name: '2010', value: 43 },
                { name: '2011', value: 90 },
                { name: '2012', value: 95 },
                { name: '2013', value: 140 }
            ]
        }
    };

    const chartconfig = {
        /*
        graph: {
            //custompalette: palete,
            bgcolor: 'none',
            //max: 64,
            //min: 0
        },
        */
        /*
        dimension: {
            height: 20
        },
        */
        margin: {
            top: 1,
            bottom: 1,
            left: 1,
            right: 1
        },
        axis: {
            showticks: false,
            showsubticks: false,
            showtext: false,
            showhortext: false,
            showvertext: false
        },
        frame: {
            bgcolor: 'none'
        },
        legend: {
            showlegends: false
        },
        label: {
            postfix: ' Бит',
            fontfamily: 'PT Sans'
        },
        effects: {
            duration: 100,
        },
        tooltip: {
            format: '%c: %v бит'
        }
    };

    var graph = new uv.chart('Line', graphdef, chartconfig);
});

