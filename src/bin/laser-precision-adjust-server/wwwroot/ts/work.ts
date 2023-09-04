declare var uv: any;

// on page loaded jquery
$(() => {
    const graphdef = {
        categories : ['uvCharts', 'Matisse', 'SocialByWay'],
        dataset : {
            'uvCharts' : [
                { name : '2008', value: 15},
                { name : '2009', value: 28},
                { name : '2010', value: 42},
                { name : '2011', value: 88},
                { name : '2012', value: 100},
                { name : '2013', value: 143}
            ],
            'Matisse' : [
                { name : '2008', value: 15},
                { name : '2009', value: 28},
                { name : '2010', value: 42},
                { name : '2011', value: 88},
                { name : '2012', value: 100},
                { name : '2013', value: 143}
            ],	
            'SocialByWay' : [
                { name : '2008', value: 15},
                { name : '2009', value: 28},
                { name : '2010', value: 42},
                { name : '2011', value: 88},
                { name : '2012', value: 100},
                { name : '2013', value: 143}
            ]
        }
    };
    
    const chartconfig = {
        graph: {
            //custompalette: palete,
            bgcolor: 'none',
            //max: 64,
            //min: 0
        },
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